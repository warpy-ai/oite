//! Native heap allocator for tscl runtime
//!
//! This module provides memory allocation for native-compiled code.
//! Design goals:
//! - Fast bump allocation for young objects
//! - Future: Mark-sweep GC for long-lived objects
//! - Interop with VM's Vec<HeapObject> during transition
//!
//! During the native compilation pivot, both the VM heap (Vec<HeapObject>)
//! and the native heap (NativeHeap) coexist. Eventually, all allocation
//! will go through NativeHeap.

use std::alloc::{self, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::object::shape::{self, Shape};

/// Property slots stored in the object itself; the rest go to `overflow`.
pub const INLINE_SLOTS: usize = 4;

/// A pointer to a heap-allocated object.
///
/// This is a wrapper around a raw pointer that:
/// - Fits in 48 bits (NaN-boxing requirement)
/// - Can be converted to/from usize for OtValue
/// - Provides type-safe access to heap objects
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct HeapPtr {
    ptr: *mut u8,
}

impl HeapPtr {
    /// Create a HeapPtr from a raw pointer.
    #[inline]
    pub fn from_ptr(ptr: *mut u8) -> Self {
        Self { ptr }
    }

    /// Create a HeapPtr from a usize (for NaN-boxing interop).
    #[inline]
    pub fn from_usize(addr: usize) -> Self {
        Self {
            ptr: addr as *mut u8,
        }
    }

    /// Get the raw pointer.
    #[inline]
    pub fn as_ptr(self) -> *mut u8 {
        self.ptr
    }

    /// Get as usize (for NaN-boxing interop).
    #[inline]
    pub fn as_usize(self) -> usize {
        self.ptr as usize
    }

    /// Create a null pointer.
    #[inline]
    pub const fn null() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
        }
    }

    /// Check if null.
    #[inline]
    pub fn is_null(self) -> bool {
        self.ptr.is_null()
    }

    /// Cast to a typed reference.
    ///
    /// # Safety
    /// The pointer must point to valid memory of type T.
    #[inline]
    pub unsafe fn as_ref<T>(self) -> &'static T {
        unsafe { &*(self.ptr as *const T) }
    }

    /// Cast to a typed mutable reference.
    ///
    /// # Safety
    /// The pointer must point to valid memory of type T.
    #[inline]
    pub unsafe fn as_mut<T>(self) -> &'static mut T {
        unsafe { &mut *(self.ptr as *mut T) }
    }
}

impl std::fmt::Debug for HeapPtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HeapPtr({:p})", self.ptr)
    }
}

impl std::fmt::Pointer for HeapPtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Pointer::fmt(&self.ptr, f)
    }
}

// =========================================================================
// Object Headers
// =========================================================================

/// Type tag for heap objects (stored in object header).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// A JavaScript-like object with string keys.
    Object = 0,
    /// A JavaScript-like array.
    Array = 1,
    /// A string (UTF-8 encoded).
    String = 2,
    /// A function closure.
    Function = 3,
    /// A ByteStream buffer (for bytecode generation).
    ByteStream = 4,
}

/// Header for all heap-allocated objects.
///
/// This is placed at the start of every heap allocation.
/// Native code reads this to determine the object type.
#[repr(C)]
pub struct ObjectHeader {
    /// Object type tag.
    pub kind: ObjectKind,
    /// GC mark bit (for future mark-sweep).
    pub marked: bool,
    /// Per-kind flags (`ObjectHeader::ASCII` for strings).
    pub flags: u8,
    /// Reserved for alignment and future use.
    pub _reserved: [u8; 5],
    /// Size of the object data (excluding header).
    pub size: u32,
}

impl ObjectHeader {
    pub const SIZE: usize = std::mem::size_of::<ObjectHeader>();
    /// A string whose bytes are all ASCII: its characters are its bytes, so
    /// indexing it needs no scan.
    pub const ASCII: u8 = 1;

    pub fn new(kind: ObjectKind, size: u32) -> Self {
        Self {
            kind,
            marked: false,
            flags: 0,
            _reserved: [0; 5],
            size,
        }
    }
}

// =========================================================================
// Native Object Layouts
// =========================================================================

/// A native string object.
///
/// Inline data follows (variable length).
/// Accessed via pointer arithmetic: (self as *const u8) + size_of::<NativeString>()
#[repr(C)]
pub struct NativeString {
    pub header: ObjectHeader,
    /// Length in bytes.
    pub len: u32,
}

impl NativeString {
    /// Get the string data as a slice.
    ///
    /// # Safety
    /// The object must be properly allocated with the correct length.
    pub unsafe fn as_str(&self) -> &str {
        unsafe {
            let data_ptr = (self as *const Self as *const u8).add(std::mem::size_of::<Self>());
            let slice = std::slice::from_raw_parts(data_ptr, self.len as usize);
            std::str::from_utf8_unchecked(slice)
        }
    }
}

/// A native array object.
#[repr(C)]
pub struct NativeArray {
    pub header: ObjectHeader,
    /// Number of elements.
    pub len: u32,
    /// Capacity (for dynamic resizing).
    pub capacity: u32,
    /// Pointer to element data (OtValue array).
    pub elements: *mut u64,
}

/// A function object: native code with the Oite calling convention (see
/// `NativeFn`), plus the environment object a closure captured.
///
/// Compiled user functions and runtime built-ins (`console.log`) both use it.
#[repr(C)]
pub struct NativeFunction {
    pub header: ObjectHeader,
    pub code: *const u8,
    /// The closure's environment (an object value), or `undefined`.
    pub env: u64,
}

/// Signature of every function callable through a `NativeFunction`:
/// `this`, the arguments, and the function's own environment.
pub type NativeFn = extern "C" fn(this: u64, argv: *const u64, argc: usize, env: u64) -> u64;

/// A native object (docs/docs/object-model.md). Its shape says which
/// property is in which slot; `object::object` reads and writes them.
#[repr(C)]
pub struct NativeObject {
    pub header: ObjectHeader,
    /// Slots allocated at `overflow`.
    pub overflow_cap: u32,
    /// The layout of the properties; null in dictionary mode.
    pub shape: *const Shape,
    /// The prototype: an object, or `undefined`.
    pub proto: u64,
    /// Slots `INLINE_SLOTS..`; in dictionary mode, the `Dict`.
    pub overflow: *mut u64,
    /// Slots `0..INLINE_SLOTS`.
    pub inline: [u64; INLINE_SLOTS],
}

// =========================================================================
// Native Heap
// =========================================================================

/// Configuration for the native heap.
pub struct HeapConfig {
    /// Initial size of the young generation (bump allocator).
    pub young_size: usize,
    /// Threshold for triggering GC.
    pub gc_threshold: usize,
}

impl Default for HeapConfig {
    fn default() -> Self {
        Self {
            young_size: 1024 * 1024,  // 1 MB
            gc_threshold: 768 * 1024, // 75% of young_size
        }
    }
}

/// The native heap allocator.
///
/// This provides fast bump allocation for young objects.
/// Long-lived objects will eventually be promoted to an old generation
/// with mark-sweep collection.
pub struct NativeHeap {
    /// Start of the current chunk.
    young_start: std::cell::Cell<*mut u8>,
    /// Current allocation pointer (bump pointer) in the current chunk.
    young_ptr: AtomicUsize,
    /// End of the current chunk.
    young_end: std::cell::Cell<*mut u8>,
    /// Every chunk allocated so far, with its size, for `Drop`.
    chunks: std::cell::RefCell<Vec<(*mut u8, usize)>>,
    /// Total bytes allocated (for stats).
    total_allocated: AtomicUsize,
    /// Configuration.
    config: HeapConfig,
}

impl Default for NativeHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeHeap {
    /// Create a new native heap with default configuration.
    pub fn new() -> Self {
        Self::with_config(HeapConfig::default())
    }

    /// Create a new native heap with custom configuration.
    pub fn with_config(config: HeapConfig) -> Self {
        let young_start = Self::alloc_chunk(config.young_size);
        let young_end = unsafe { young_start.add(config.young_size) };

        Self {
            young_start: std::cell::Cell::new(young_start),
            young_ptr: AtomicUsize::new(young_start as usize),
            young_end: std::cell::Cell::new(young_end),
            chunks: std::cell::RefCell::new(vec![(young_start, config.young_size)]),
            total_allocated: AtomicUsize::new(0),
            config,
        }
    }

    fn alloc_chunk(size: usize) -> *mut u8 {
        let layout = Layout::from_size_align(size, 8).unwrap();
        let chunk = unsafe { alloc::alloc(layout) };
        if chunk.is_null() {
            alloc::handle_alloc_error(layout);
        }
        chunk
    }

    /// Allocate memory for an object of the given size.
    ///
    /// When the current chunk is full, a new chunk is added. Nothing is
    /// reclaimed yet (#123), so memory grows with every allocation, but an
    /// allocation never fails short of the system running out of memory.
    pub fn alloc(&self, size: usize) -> Option<HeapPtr> {
        let total_size = ObjectHeader::SIZE + size;
        let aligned_size = (total_size + 7) & !7; // 8-byte alignment

        let current = self.young_ptr.load(Ordering::Relaxed);
        let mut start = current;
        if current + aligned_size > self.young_end.get() as usize {
            let chunk_size = self.config.young_size.max(aligned_size);
            let chunk = Self::alloc_chunk(chunk_size);
            self.chunks.borrow_mut().push((chunk, chunk_size));
            self.young_start.set(chunk);
            self.young_end.set(unsafe { chunk.add(chunk_size) });
            start = chunk as usize;
        }
        self.young_ptr
            .store(start + aligned_size, Ordering::Relaxed);
        self.total_allocated
            .fetch_add(aligned_size, Ordering::Relaxed);
        Some(HeapPtr::from_usize(start))
    }

    /// Allocate and initialize a string.
    pub fn alloc_string(&self, s: &str) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeString>() - ObjectHeader::SIZE + s.len();
        let ptr = self.alloc(data_size)?;

        unsafe {
            let header = ptr.as_mut::<ObjectHeader>();
            *header = ObjectHeader::new(ObjectKind::String, data_size as u32);
            if s.is_ascii() {
                header.flags |= ObjectHeader::ASCII;
            }

            let string_obj = ptr.as_mut::<NativeString>();
            string_obj.len = s.len() as u32;

            // Copy string data
            let data_ptr = (ptr.as_ptr() as *mut u8).add(std::mem::size_of::<NativeString>());
            std::ptr::copy_nonoverlapping(s.as_ptr(), data_ptr, s.len());
        }

        Some(ptr)
    }

    /// Allocate and initialize an empty object.
    pub fn alloc_object(&self) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeObject>() - ObjectHeader::SIZE;
        let ptr = self.alloc(data_size)?;

        unsafe {
            let header = ptr.as_mut::<ObjectHeader>();
            *header = ObjectHeader::new(ObjectKind::Object, data_size as u32);

            let obj = ptr.as_mut::<NativeObject>();
            obj.overflow_cap = 0;
            obj.shape = shape::empty();
            obj.proto = super::abi::OtValue::undefined().to_bits();
            obj.overflow = std::ptr::null_mut();
        }

        Some(ptr)
    }

    /// Allocate and initialize an array with given capacity.
    pub fn alloc_array(&self, capacity: usize) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeArray>() - ObjectHeader::SIZE;
        let ptr = self.alloc(data_size)?;
        // Element storage is always allocated, so it can be grown with realloc.
        let capacity = capacity.max(4);

        unsafe {
            let header = ptr.as_mut::<ObjectHeader>();
            *header = ObjectHeader::new(ObjectKind::Array, data_size as u32);

            let arr = ptr.as_mut::<NativeArray>();
            arr.len = 0;
            arr.capacity = capacity as u32;
            // Allocate element storage separately
            let layout = Layout::array::<u64>(capacity).unwrap();
            arr.elements = alloc::alloc(layout) as *mut u64;
        }

        Some(ptr)
    }

    /// Allocate a function object pointing at native code.
    pub fn alloc_function(&self, code: *const u8, env: u64) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeFunction>() - ObjectHeader::SIZE;
        let ptr = self.alloc(data_size)?;
        unsafe {
            let header = ptr.as_mut::<ObjectHeader>();
            *header = ObjectHeader::new(ObjectKind::Function, data_size as u32);
            let f = ptr.as_mut::<NativeFunction>();
            f.code = code;
            f.env = env;
        }
        Some(ptr)
    }

    /// Get the total bytes allocated.
    pub fn total_allocated(&self) -> usize {
        self.total_allocated.load(Ordering::Relaxed)
    }

    /// Get the bytes remaining in the current chunk.
    pub fn bytes_remaining(&self) -> usize {
        let current = self.young_ptr.load(Ordering::Relaxed);
        self.young_end.get() as usize - current
    }

    /// Check if GC should be triggered.
    pub fn should_gc(&self) -> bool {
        self.bytes_remaining() < (self.config.young_size - self.config.gc_threshold)
    }

    /// Reset the heap (for testing): back to the start of the first chunk.
    pub fn reset(&self) {
        let (first, size) = self.chunks.borrow()[0];
        self.young_start.set(first);
        self.young_end.set(unsafe { first.add(size) });
        self.young_ptr.store(first as usize, Ordering::SeqCst);
        self.total_allocated.store(0, Ordering::SeqCst);
    }
}

impl Drop for NativeHeap {
    fn drop(&mut self) {
        for &(chunk, size) in self.chunks.borrow().iter() {
            let layout = Layout::from_size_align(size, 8).unwrap();
            unsafe {
                alloc::dealloc(chunk, layout);
            }
        }
    }
}

// Thread-local heap for single-threaded use
thread_local! {
    static HEAP: NativeHeap = NativeHeap::new();
}

/// Get the thread-local heap.
pub fn heap() -> &'static NativeHeap {
    // Safety: We're returning a reference to thread-local storage
    // This is safe because:
    // 1. The heap is initialized lazily on first access
    // 2. The reference is only valid for the current thread
    // 3. We never move the heap
    HEAP.with(|h| unsafe { &*(h as *const NativeHeap) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layouts native code and the runtime agree on. A change here is
    /// an ABI change: update this test with it.
    #[test]
    fn object_layouts() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<ObjectHeader>(), 12);
        assert_eq!(offset_of!(ObjectHeader, kind), 0);
        assert_eq!(offset_of!(ObjectHeader, flags), 2);
        assert_eq!(offset_of!(ObjectHeader, size), 8);

        assert_eq!(size_of::<NativeString>(), 16);
        assert_eq!(offset_of!(NativeString, len), 12);

        assert_eq!(size_of::<NativeArray>(), 32);
        assert_eq!(offset_of!(NativeArray, len), 12);
        assert_eq!(offset_of!(NativeArray, capacity), 16);
        assert_eq!(offset_of!(NativeArray, elements), 24);

        assert_eq!(size_of::<NativeFunction>(), 32);
        assert_eq!(offset_of!(NativeFunction, code), 16);
        assert_eq!(offset_of!(NativeFunction, env), 24);

        assert_eq!(size_of::<NativeObject>(), 40 + 8 * INLINE_SLOTS);
        assert_eq!(offset_of!(NativeObject, overflow_cap), 12);
        assert_eq!(offset_of!(NativeObject, shape), 16);
        assert_eq!(offset_of!(NativeObject, proto), 24);
        assert_eq!(offset_of!(NativeObject, overflow), 32);
        assert_eq!(offset_of!(NativeObject, inline), 40);
    }

    #[test]
    fn test_alloc_object() {
        let heap = NativeHeap::new();
        let ptr = heap.alloc_object().expect("allocation failed");
        assert!(!ptr.is_null());

        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            assert_eq!(header.kind, ObjectKind::Object);
        }
    }

    #[test]
    fn test_alloc_string() {
        let heap = NativeHeap::new();
        let ptr = heap.alloc_string("hello").expect("allocation failed");

        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            assert_eq!(header.kind, ObjectKind::String);

            let s = ptr.as_ref::<NativeString>();
            assert_eq!(s.as_str(), "hello");
        }
    }

    #[test]
    fn test_alloc_array() {
        let heap = NativeHeap::new();
        let ptr = heap.alloc_array(10).expect("allocation failed");

        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            assert_eq!(header.kind, ObjectKind::Array);

            let arr = ptr.as_ref::<NativeArray>();
            assert_eq!(arr.len, 0);
            assert_eq!(arr.capacity, 10);
        }
    }

    #[test]
    fn test_heap_ptr_roundtrip() {
        let addr: usize = 0x1234_5678_9ABC;
        let ptr = HeapPtr::from_usize(addr);
        assert_eq!(ptr.as_usize(), addr);
    }
}
