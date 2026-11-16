//! Classes: construction, `super`, private names, `in` and `instanceof`.

use super::*;

/// Whether the object at `ptr` is a class (marked `__class__`).
pub(super) fn is_class(ptr: HeapPtr) -> bool {
    own_value(ptr, atom::CLASS) == Some(boolean(true))
}

/// `[class C]` or `[class C extends P]`, as Node prints a class.
pub(super) fn class_summary(ptr: HeapPtr) -> String {
    let name_of = |p: HeapPtr| match lookup(p, "name").and_then(|n| as_str(OtValue::from_bits(n))) {
        Some(n) => n.to_string(),
        None => "(anonymous)".to_string(),
    };
    let parent = object(OtValue::from_bits(obj(ptr).proto))
        .filter(|(p, kind)| *kind == ObjectKind::Object && is_class(*p));
    match parent {
        Some((p, _)) => format!("[class {} extends {}]", name_of(ptr), name_of(p)),
        None => format!("[class {}]", name_of(ptr)),
    }
}

/// The result of `new`: what the constructor returned if it is an object,
/// otherwise `this`.
pub(super) fn construct_result(result: u64, this: u64) -> u64 {
    if result == EXCEPTION_BITS {
        return result;
    }
    match object(OtValue::from_bits(result)) {
        Some((_, kind)) if kind != ObjectKind::String => result,
        _ => this,
    }
}

/// Run `ctor` (a class, or a plain function) as a constructor on `this`.
pub(super) fn run_constructor(ctor: u64, this: u64, argv: *const u64, argc: usize) -> Option<u64> {
    let c = OtValue::from_bits(ctor);
    let code = match object(c)? {
        (ptr, ObjectKind::Object) if is_class(ptr) => lookup_atom(ptr, atom::CONSTRUCTOR)?,
        (_, ObjectKind::Function) => ctor,
        _ => return None,
    };
    Some(construct_result(ot_call(code, this, argv, argc), this))
}

/// A new instance of `class`: an object whose prototype is `proto`, then
/// the constructor `code` on it. The instance is allocated with room for
/// as many properties as the class's instances have had when their
/// constructor returned.
fn construct_class(
    class: &mut NativeObject,
    code: u64,
    proto: Option<u64>,
    argv: *const u64,
    argc: usize,
) -> u64 {
    // Never fewer than the default: properties are also added to instances
    // after the constructor.
    let slots = (class.header.instance_slots as usize).max(INLINE_SLOTS);
    let Some(this_ptr) = heap().alloc_object_sized(slots) else {
        return undefined();
    };
    let this = OtValue::pointer(this_ptr).to_bits();
    if let Some(proto) = proto {
        obj(this_ptr).proto = proto;
        mark_prototype(proto);
    }
    let result = construct_result(ot_call(code, this, argv, argc), this);
    let instance = obj(this_ptr);
    if !instance.shape.is_null() {
        let props = unsafe { &*instance.shape }
            .props
            .len()
            .min(MAX_INLINE_SLOTS);
        if props > class.header.instance_slots as usize {
            class.header.instance_slots = props as u8;
        }
    }
    result
}

/// `new ctor(...args)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_construct(ctor: u64, argv: *const u64, argc: usize) -> u64 {
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor))
        && is_class(ptr)
        && let Some(code) = lookup_atom(ptr, atom::CONSTRUCTOR)
    {
        let proto = lookup_atom(ptr, atom::PROTOTYPE);
        return construct_class(obj(ptr), code, proto, argv, argc);
    }
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor))
        && lookup_atom(ptr, atom::PROMISE_CTOR).is_some()
    {
        let executor = unsafe { args(argv, argc) }
            .first()
            .copied()
            .unwrap_or_else(undefined);
        return construct_promise(executor);
    }
    let this = ot_alloc_object();
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor))
        && let Some(proto) = lookup_atom(ptr, atom::PROTOTYPE)
    {
        set_property(OtValue::from_bits(this), "__proto__", proto);
    }
    match run_constructor(ctor, this, argv, argc) {
        Some(r) => r,
        None => throw_error(
            "TypeError",
            &format!("{} is not a constructor", inspect(OtValue::from_bits(ctor))),
        ),
    }
}

/// `new ctor(...args)` through the site's cache: a class of the cached
/// shape has its marker, constructor and prototype in known slots.
#[unsafe(no_mangle)]
pub extern "C" fn ot_construct_ic(
    ctor: u64,
    argv: *const u64,
    argc: usize,
    cache: *mut ic::NewCache,
) -> u64 {
    let c = unsafe { &mut *cache };
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor)) {
        let class = obj(ptr);
        if class.shape as u64 == c.shape {
            let raw = class as *mut NativeObject;
            let at = |offset| unsafe { *ic::slot_at(raw, offset) };
            if at(c.class_offset) == boolean(true) {
                let (code, proto) = (at(c.ctor_offset), at(c.proto_offset));
                return construct_class(class, code, Some(proto), argv, argc);
            }
        } else if !class.shape.is_null() {
            // Remember the slots, when all three are the class's own data
            // properties.
            let shape = unsafe { &*class.shape };
            let data = |key| match shape.find(key) {
                Some((i, flags)) if flags & ACCESSOR == 0 => {
                    Some(ic::slot_offset(i, shape.inline_slots as usize))
                }
                _ => None,
            };
            if let (Some(class_offset), Some(ctor_offset), Some(proto_offset)) = (
                data(atom::CLASS),
                data(atom::CONSTRUCTOR),
                data(atom::PROTOTYPE),
            ) {
                c.shape = class.shape as u64;
                c.class_offset = class_offset;
                c.ctor_offset = ctor_offset;
                c.proto_offset = proto_offset;
            }
        }
    }
    ot_construct(ctor, argv, argc)
}

/// `super(...args)`: the parent class's constructor on the current `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call_super(parent: u64, this: u64, argv: *const u64, argc: usize) -> u64 {
    match run_constructor(parent, this, argv, argc) {
        Some(r) => r,
        None => throw_error("TypeError", "Super constructor is not a constructor"),
    }
}

/// `super.key`: looked up from `base`; a getter runs with `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_super_prop(this: u64, base: u64, key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(base)) else {
        return undefined();
    };
    match lookup(ptr, &format!("getter:{key}")) {
        Some(g)
            if matches!(
                object(OtValue::from_bits(g)),
                Some((_, ObjectKind::Function))
            ) =>
        {
            ot_call(g, this, std::ptr::null(), 0)
        }
        _ => lookup(ptr, key).unwrap_or_else(undefined),
    }
}

/// `#x` from a private member's key (`#x@<class>`), for messages.
pub(super) fn private_name(key: &str) -> &str {
    key.split('@').next().unwrap_or(key)
}

/// `obj.#x`: a TypeError if the object lacks the private member.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_private(obj: u64, key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        if let Some(v) = lookup(ptr, key) {
            return v;
        }
        if let Some(getter) = lookup(ptr, &format!("getter:{key}")) {
            return ot_call(getter, obj, std::ptr::null(), 0);
        }
    }
    throw_error(
        "TypeError",
        &format!(
            "Cannot read private member {} from an object whose class did not declare it",
            private_name(key)
        ),
    )
}

/// `obj.#x = value`: a TypeError if the object lacks the private field.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_private(obj: u64, key: *const u8, key_len: usize, value: u64) -> u64 {
    let key = unsafe { name(key, key_len) };
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        let key_atom = atom::atom(key);
        if own_value(ptr, key_atom).is_some() {
            object::set_own(self::obj(ptr), key_atom, 0, value);
            return undefined();
        }
        if let Some(setter) = lookup(ptr, &format!("setter:{key}")) {
            let r = ot_call(setter, obj, &value, 1);
            return if r == EXCEPTION_BITS { r } else { undefined() };
        }
    }
    throw_error(
        "TypeError",
        &format!(
            "Cannot write private member {} to an object whose class did not declare it",
            private_name(key)
        ),
    )
}

/// `key in obj`: the object or its prototype chain has the property (a
/// getter or setter counts).
#[unsafe(no_mangle)]
pub extern "C" fn ot_in(key: u64, obj: u64) -> u64 {
    let key = value_to_string(OtValue::from_bits(key));
    match object(OtValue::from_bits(obj)) {
        Some((ptr, ObjectKind::Object)) => boolean(
            lookup(ptr, &key).is_some()
                || lookup(ptr, &format!("getter:{key}")).is_some()
                || lookup(ptr, &format!("setter:{key}")).is_some(),
        ),
        Some((ptr, ObjectKind::Array)) => boolean(
            key == "length"
                || key
                    .parse::<usize>()
                    .is_ok_and(|i| i < array(ptr).len as usize),
        ),
        _ => throw_error(
            "TypeError",
            &format!(
                "Cannot use 'in' operator to search for '{}' in {}",
                key,
                inspect(OtValue::from_bits(obj))
            ),
        ),
    }
}

/// `value instanceof ctor`: whether `ctor.prototype` is on `value`'s
/// prototype chain. (A plain function counts as matching, as on the VM.)
#[unsafe(no_mangle)]
pub extern "C" fn ot_instanceof(value: u64, ctor: u64) -> u64 {
    // `x instanceof Object` holds for every object.
    if global("Object") == Some(ctor) {
        return boolean(
            matches!(object(OtValue::from_bits(value)), Some((_, kind)) if kind != ObjectKind::String),
        );
    }
    let proto = match object(OtValue::from_bits(ctor)) {
        Some((_, ObjectKind::Function)) => return boolean(true),
        Some((ptr, ObjectKind::Object)) => lookup_atom(ptr, atom::PROTOTYPE),
        _ => None,
    };
    let Some(proto) = proto else {
        return boolean(false);
    };
    let mut cur = OtValue::from_bits(value);
    for _ in 0..1000 {
        let Some((ptr, ObjectKind::Object)) = object(cur) else {
            return boolean(false);
        };
        let p = obj(ptr).proto;
        if p == proto {
            return boolean(true);
        }
        cur = OtValue::from_bits(p);
    }
    boolean(false)
}
