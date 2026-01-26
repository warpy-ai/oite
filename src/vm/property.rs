use crate::vm::VM;
use crate::vm::value::{HeapData, HeapObject, JsValue};

const MAX_PROTO_DEPTH: usize = 100;

pub fn get_prop_with_proto_chain(vm: &VM, obj_ptr: usize, name: &str) -> JsValue {
    let mut current_ptr = Some(obj_ptr);
    let mut depth = 0;

    while let Some(ptr) = current_ptr {
        if depth > MAX_PROTO_DEPTH {
            break;
        }
        depth += 1;

        if let Some(HeapObject {
            data: HeapData::Object(props),
        }) = vm.heap.get(ptr)
        {
            if let Some(val) = props.get(name) {
                return val.clone();
            }

            if let Some(JsValue::Object(proto_ptr)) = props.get("__proto__") {
                current_ptr = Some(*proto_ptr);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    JsValue::Undefined
}

pub fn find_setter_with_proto_chain(
    vm: &VM,
    obj_ptr: usize,
    name: &str,
) -> Option<(usize, Option<usize>)> {
    let setter_name = format!("setter:{}", name);
    let mut current_ptr = Some(obj_ptr);
    let mut depth = 0;

    while let Some(ptr) = current_ptr {
        if depth > MAX_PROTO_DEPTH {
            break;
        }
        depth += 1;

        if let Some(HeapObject {
            data: HeapData::Object(props),
        }) = vm.heap.get(ptr)
        {
            if let Some(setter_val) = props.get(&setter_name)
                && let JsValue::Function { address, env } = setter_val
            {
                return Some((*address, *env));
            }

            if let Some(JsValue::Object(proto_ptr)) = props.get("__proto__") {
                current_ptr = Some(*proto_ptr);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    None
}
