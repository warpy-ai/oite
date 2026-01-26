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

/// Simple property storage using a Vec instead of HashMap to avoid hashbrown dependency.
/// This is a tradeoff: O(n) lookup but no external dependencies.
pub type PropertyMap = Vec<(String, u64)>;

/// A pointer to a heap-allocated object.
///
/// This is a wrapper around a raw pointer that:
/// - Fits in 48 bits (NaN-boxing requirement)
/// - Can be converted to/from usize for TsclValue
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
    /// Reserved for alignment and future use.
    pub _reserved: [u8; 6],
    /// Size of the object data (excluding header).
    pub size: u32,
}

impl ObjectHeader {
    pub const SIZE: usize = std::mem::size_of::<ObjectHeader>();

    pub fn new(kind: ObjectKind, size: u32) -> Self {
        Self {
            kind,
            marked: false,
            _reserved: [0; 6],
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
    /// Pointer to element data (TsclValue array).
    pub elements: *mut u64,
}

/// A native object (key-value map).
///
/// For simplicity, we use a Rust HashMap internally.
/// A production implementation would use inline slots + overflow hash.
#[repr(C)]
pub struct NativeObject {
    pub header: ObjectHeader,
    /// Pointer to the property map (Vec of key-value pairs).
    /// We store this as a raw pointer to avoid Rust's ownership rules
    /// in the native runtime. Uses Vec instead of HashMap to avoid hashbrown dep.
    pub properties: *mut PropertyMap,
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
    /// Start of the young generation.
    young_start: *mut u8,
    /// Current allocation pointer (bump pointer).
    young_ptr: AtomicUsize,
    /// End of the young generation.
    young_end: *mut u8,
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
        let layout = Layout::from_size_align(config.young_size, 8).unwrap();
        let young_start = unsafe { alloc::alloc(layout) };
        if young_start.is_null() {
            panic!("Failed to allocate native heap");
        }
        let young_end = unsafe { young_start.add(config.young_size) };

        Self {
            young_start,
            young_ptr: AtomicUsize::new(young_start as usize),
            young_end,
            total_allocated: AtomicUsize::new(0),
            config,
        }
    }

    /// Allocate memory for an object of the given size.
    ///
    /// Returns None if allocation fails (heap full, need GC).
    pub fn alloc(&self, size: usize) -> Option<HeapPtr> {
        let total_size = ObjectHeader::SIZE + size;
        let aligned_size = (total_size + 7) & !7; // 8-byte alignment

        loop {
            let current = self.young_ptr.load(Ordering::Relaxed);
            let new_ptr = current + aligned_size;

            if new_ptr > self.young_end as usize {
                // Out of space - need GC
                return None;
            }

            // Try to bump the pointer atomically
            match self.young_ptr.compare_exchange_weak(
                current,
                new_ptr,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.total_allocated
                        .fetch_add(aligned_size, Ordering::Relaxed);
                    return Some(HeapPtr::from_usize(current));
                }
                Err(_) => continue, // Retry
            }
        }
    }

    /// Allocate and initialize a string.
    pub fn alloc_string(&self, s: &str) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeString>() - ObjectHeader::SIZE + s.len();
        let ptr = self.alloc(data_size)?;

        unsafe {
            let header = ptr.as_mut::<ObjectHeader>();
            *header = ObjectHeader::new(ObjectKind::String, data_size as u32);

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
            // Allocate the PropertyMap separately (it lives outside the bump allocator)
            obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
        }

        Some(ptr)
    }

    /// Allocate and initialize an array with given capacity.
    pub fn alloc_array(&self, capacity: usize) -> Option<HeapPtr> {
        let data_size = std::mem::size_of::<NativeArray>() - ObjectHeader::SIZE;
        let ptr = self.alloc(data_size)?;

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

    /// Get the total bytes allocated.
    pub fn total_allocated(&self) -> usize {
        self.total_allocated.load(Ordering::Relaxed)
    }

    /// Get the bytes remaining in the young generation.
    pub fn bytes_remaining(&self) -> usize {
        let current = self.young_ptr.load(Ordering::Relaxed);
        self.young_end as usize - current
    }

    /// Check if GC should be triggered.
    pub fn should_gc(&self) -> bool {
        self.bytes_remaining() < (self.config.young_size - self.config.gc_threshold)
    }

    /// Reset the heap (for testing).
    pub fn reset(&self) {
        self.young_ptr
            .store(self.young_start as usize, Ordering::SeqCst);
        self.total_allocated.store(0, Ordering::SeqCst);
    }
}

impl Drop for NativeHeap {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.config.young_size, 8).unwrap();
        unsafe {
            alloc::dealloc(self.young_start, layout);
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
