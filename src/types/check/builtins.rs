//! The members of strings, numbers and arrays, and the global functions
//! whose result type is fixed.

use super::*;

/// A function taking any arguments and returning `ret`.
fn returns(ret: Ty) -> Ty {
    Ty::function(
        vec![Param {
            name: "args".to_string(),
            ty: Ty::Any,
            optional: false,
            rest: true,
        }],
        ret,
    )
}

/// The type of `string.name`, when it is known.
pub(super) fn string_member(name: &str) -> Option<Ty> {
    Some(match name {
        "length" => Ty::Number,
        "charAt" | "slice" | "substring" | "substr" | "toUpperCase" | "toLowerCase" | "trim"
        | "trimStart" | "trimEnd" | "padStart" | "padEnd" | "repeat" | "replace" | "replaceAll"
        | "concat" | "toString" | "normalize" => returns(Ty::String),
        "charCodeAt" | "indexOf" | "lastIndexOf" | "localeCompare" | "search" => {
            returns(Ty::Number)
        }
        "includes" | "startsWith" | "endsWith" => returns(Ty::Boolean),
        "split" => returns(Ty::array(Ty::String)),
        _ => return None,
    })
}

/// The type of `number.name`, when it is known.
pub(super) fn number_member(name: &str) -> Option<Ty> {
    Some(match name {
        "toFixed" | "toString" | "toPrecision" | "toExponential" => returns(Ty::String),
        _ => return None,
    })
}

/// The type of `array.name` for an array of `elem`, when it is known.
pub(super) fn array_member(elem: &Ty, name: &str) -> Option<Ty> {
    let same = || Ty::array(elem.clone());
    let maybe = || Ty::union([elem.clone(), Ty::Undefined]);
    Some(match name {
        "length" => Ty::Number,
        // What is pushed must be an element.
        "push" | "unshift" => Ty::function(
            vec![Param {
                name: "items".to_string(),
                ty: elem.clone(),
                optional: false,
                rest: true,
            }],
            Ty::Number,
        ),
        "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex" => returns(Ty::Number),
        "pop" | "shift" | "find" | "findLast" | "at" => returns(maybe()),
        "join" | "toString" => returns(Ty::String),
        "includes" | "some" | "every" => returns(Ty::Boolean),
        "slice" | "concat" | "filter" | "reverse" | "sort" | "splice" | "fill" => returns(same()),
        "map" | "flatMap" | "flat" => returns(Ty::array(Ty::Any)),
        "forEach" => returns(Ty::Void),
        _ => return None,
    })
}

/// The type of the global `name`, when its type is fixed and the program
/// has not declared the name itself.
pub(super) fn global(name: &str) -> Option<Ty> {
    Some(match name {
        "undefined" => Ty::Undefined,
        "NaN" | "Infinity" => Ty::Number,
        "parseInt" | "parseFloat" | "Number" => returns(Ty::Number),
        "String" => returns(Ty::String),
        "Boolean" | "isNaN" | "isFinite" => returns(Ty::Boolean),
        _ => return None,
    })
}

/// The type of `Math.name`.
pub(super) fn math_member(name: &str) -> Ty {
    match name {
        "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "SQRT2" | "SQRT1_2" => Ty::Number,
        _ => returns(Ty::Number),
    }
}
