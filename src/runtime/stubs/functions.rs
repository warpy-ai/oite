//! Functions are objects: the properties a program gives a function, its
//! `name`, `length` and `prototype`, `call`, `apply` and `bind`, and the
//! bound functions `bind` makes.
//!
//! A function's own properties are kept in an object of their own,
//! `NativeFunction::props`, made when the first one is set. What the
//! compiler knows of each compiled function (`name`, `length`, whether it
//! has a `prototype`) is registered by its code when the program starts.

use super::*;
use builtins::{Method, Receiver};

/// What the compiler says of a compiled function.
struct Info {
    name: &'static str,
    length: usize,
    prototype: bool,
}

thread_local! {
    /// By code address.
    static INFO: RefCell<HashMap<usize, Info>> = RefCell::new(HashMap::new());
}

/// Register what the compiled function whose code is `code` is: its
/// `name`, its `length`, and whether it has a `prototype` (`prototype`
/// is 0 or 1). At program start, before any code runs.
#[unsafe(no_mangle)]
pub extern "C" fn ot_register_function(
    code: *const u8,
    name: *const u8,
    name_len: usize,
    length: usize,
    prototype: usize,
) {
    let name = unsafe { super::name(name, name_len) };
    let info = Info {
        name: Box::leak(name.to_string().into_boxed_str()),
        length,
        prototype: prototype != 0,
    };
    INFO.with(|i| i.borrow_mut().insert(code as usize, info));
}

pub(super) fn native_fn(ptr: HeapPtr) -> &'static mut NativeFunction {
    unsafe { ptr.as_mut::<NativeFunction>() }
}

/// The function at `ptr`'s own properties, if it has any.
fn props(ptr: HeapPtr) -> Option<HeapPtr> {
    match object(OtValue::from_bits(native_fn(ptr).props))? {
        (p, ObjectKind::Object) => Some(p),
        _ => None,
    }
}

/// ... made if it has none.
fn props_or_new(ptr: HeapPtr) -> HeapPtr {
    if let Some(p) = props(ptr) {
        return p;
    }
    let bag = ot_alloc_object();
    native_fn(ptr).props = bag;
    object(OtValue::from_bits(bag)).expect("an object").0
}

/// The function at `ptr`'s own property `key`.
fn own(ptr: HeapPtr, key: Atom) -> Option<u64> {
    own_value(props(ptr)?, key)
}

/// A bound function: (target, `this`, the bound arguments), from its
/// environment (an array of the three).
fn bound(ptr: HeapPtr) -> Option<(u64, u64, &'static [u64])> {
    let f = native_fn(ptr);
    if f.code != ot_bound_function as *const u8 {
        return None;
    }
    let (env, ObjectKind::Array) = object(OtValue::from_bits(f.env))? else {
        return None;
    };
    let arr = array(env);
    let items = unsafe { std::slice::from_raw_parts(arr.elements, arr.len as usize) };
    Some((items[0], items[1], &items[2..]))
}

/// A bound function called (`f.bind(this, ...args)`): its target, with the
/// bound `this` and the bound arguments first. `env` is the array of the
/// target, `this` and the bound arguments.
pub extern "C" fn ot_bound_function(_this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let Some((ptr, ObjectKind::Array)) = object(OtValue::from_bits(env)) else {
        return undefined();
    };
    let arr = array(ptr);
    let items = unsafe { std::slice::from_raw_parts(arr.elements, arr.len as usize) };
    let mut all = items[2..].to_vec();
    all.extend_from_slice(unsafe { args(argv, argc) });
    ot_call(items[0], items[1], all.as_ptr(), all.len())
}

/// `target.bind(this, ...args)`.
fn bind(target: u64, a: &[u64]) -> u64 {
    let env = ot_alloc_array(a.len().max(1) + 1);
    let (ptr, _) = object(OtValue::from_bits(env)).expect("an array");
    array_push(array(ptr), target);
    array_push(array(ptr), a.first().copied().unwrap_or_else(undefined));
    for &v in a.iter().skip(1) {
        array_push(array(ptr), v);
    }
    ot_make_closure(ot_bound_function as *const u8, env)
}

/// `new` on `ctor`: through any number of binds, its target, with the
/// bound arguments before `a`. `None`: `ctor` is not a bound function.
pub(super) fn unbind_for_new(ctor: u64, a: &[u64]) -> Option<(u64, Vec<u64>)> {
    let (mut target, mut all) = (ctor, a.to_vec());
    while let Some((ptr, ObjectKind::Function)) = object(OtValue::from_bits(target))
        && let Some((t, _, bound_args)) = bound(ptr)
    {
        all.splice(0..0, bound_args.iter().copied());
        target = t;
    }
    (target != ctor).then_some((target, all))
}

/// The function at `ptr`'s `name`.
pub(super) fn function_name(ptr: HeapPtr) -> String {
    let f = native_fn(ptr);
    if let Some((target, _, _)) = bound(ptr) {
        let target = match object(OtValue::from_bits(target)) {
            Some((t, ObjectKind::Function)) => function_name(t),
            _ => String::new(),
        };
        return format!("bound {target}");
    }
    if f.code == ot_builtin_method as *const u8 {
        let bits = OtValue::from_bits(f.env).as_number_unchecked() as u32;
        return Method::from_bits(bits).map_or("", |m| m.name()).to_string();
    }
    INFO.with(|i| {
        i.borrow()
            .get(&(f.code as usize))
            .map(|info| info.name.to_string())
    })
    .unwrap_or_default()
}

/// The function at `ptr`'s `length`.
fn function_length(ptr: HeapPtr) -> f64 {
    if let Some((target, _, bound_args)) = bound(ptr) {
        let target = match object(OtValue::from_bits(target)) {
            Some((t, ObjectKind::Function)) => function_length(t),
            _ => 0.0,
        };
        return builtins::bound_length(target, bound_args.len());
    }
    let code = native_fn(ptr).code as usize;
    INFO.with(|i| i.borrow().get(&code).map_or(0, |info| info.length)) as f64
}

/// Whether the function at `ptr` has a `prototype`: a `function` has one,
/// and so does any function a program gave one.
fn has_prototype(ptr: HeapPtr) -> bool {
    let code = native_fn(ptr).code as usize;
    INFO.with(|i| i.borrow().get(&code).is_some_and(|info| info.prototype))
        || own(ptr, atom::PROTOTYPE).is_some()
}

/// `f.prototype`, made the first time it is read (`{ constructor: f }`);
/// `undefined` for a function without one.
pub(super) fn function_prototype(ptr: HeapPtr) -> u64 {
    if let Some(proto) = own(ptr, atom::PROTOTYPE) {
        return proto;
    }
    if !has_prototype(ptr) {
        return undefined();
    }
    let proto = ot_alloc_object();
    let f = OtValue::pointer(ptr).to_bits();
    set_property_atom(OtValue::from_bits(proto), atom::CONSTRUCTOR, f);
    object::set_own(obj(props_or_new(ptr)), atom::PROTOTYPE, 0, proto);
    proto
}

/// The `prototype` of the function at `ptr` if it has been made: a
/// function's is made when first read, so one never read is on no
/// object's chain (`instanceof`).
pub(super) fn made_prototype(ptr: HeapPtr) -> Option<u64> {
    let ptr = match bound(ptr) {
        Some((target, _, _)) => match object(OtValue::from_bits(target)) {
            Some((t, ObjectKind::Function)) => return made_prototype(t),
            _ => return None,
        },
        None => ptr,
    };
    own(ptr, atom::PROTOTYPE)
}

/// `f[key]`: its own property, then `name`, `length`, `prototype`, and the
/// methods of `Function.prototype`.
pub(super) fn get_function_property(ptr: HeapPtr, key: Atom) -> u64 {
    if let Some(v) = own(ptr, key) {
        return v;
    }
    match key {
        atom::PROTOTYPE => function_prototype(ptr),
        atom::LENGTH => number(function_length(ptr)),
        atom::NAME => new_string(&function_name(ptr)),
        _ => builtin_method(Receiver::Function, atom::name(key)).unwrap_or_else(undefined),
    }
}

/// `f[key] = value`. `name` and `length` cannot be written (outside strict
/// mode the write is ignored).
pub(super) fn set_function_property(ptr: HeapPtr, key: Atom, value: u64) {
    if key == atom::LENGTH || key == atom::NAME {
        return;
    }
    let bag = props_or_new(ptr);
    set_property_atom(OtValue::pointer(bag), key, value);
}

/// `delete f[key]`.
pub(super) fn delete_function_property(ptr: HeapPtr, key: Atom) {
    if let Some(bag) = props(ptr) {
        object::remove_own(obj(bag), key);
    }
}

/// `key in f`.
pub(super) fn function_has(ptr: HeapPtr, key: &str) -> bool {
    let k = atom::atom(key);
    own(ptr, k).is_some()
        || matches!(key, "name" | "length")
        || (k == atom::PROTOTYPE && has_prototype(ptr))
        || Method::find(Receiver::Function, key).is_some()
}

/// The properties `Object.keys`, `for...in`, spread and `console.log` list:
/// the ones a program set (not `prototype`).
pub(super) fn function_entries(ptr: HeapPtr) -> Vec<(&'static str, u64)> {
    let Some(bag) = props(ptr) else {
        return Vec::new();
    };
    visible_data(bag)
        .into_iter()
        .filter(|(k, _)| !builtins::is_function_key(k))
        .collect()
}

/// `f.call(...)`, `f.apply(...)` or `f.bind(...)`, unless the function has
/// a property of that name of its own; `None` for any other name.
pub(super) fn function_method(recv: u64, key: &str, a: &[u64]) -> Option<u64> {
    let (ptr, ObjectKind::Function) = object(OtValue::from_bits(recv))? else {
        return None;
    };
    if !matches!(key, "call" | "apply" | "bind")
        || props(ptr).is_some_and(|bag| own_value(bag, atom::atom(key)).is_some())
    {
        return None;
    }
    let this = a.first().copied().unwrap_or_else(undefined);
    Some(match key {
        "call" => {
            let rest = a.get(1..).unwrap_or(&[]);
            ot_call(recv, this, rest.as_ptr(), rest.len())
        }
        "apply" => {
            let list: Vec<u64> = match a.get(1).and_then(|v| object(OtValue::from_bits(*v))) {
                Some((ptr, ObjectKind::Array)) => {
                    let arr = array(ptr);
                    (0..arr.len as usize)
                        .map(|i| unsafe { *arr.elements.add(i) })
                        .collect()
                }
                _ => Vec::new(),
            };
            ot_call(recv, this, list.as_ptr(), list.len())
        }
        _ => bind(recv, a),
    })
}
