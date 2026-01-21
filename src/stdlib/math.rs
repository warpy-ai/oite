use crate::vm::value::JsValue;
use crate::vm::VM;

pub fn native_math_abs(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.abs())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_floor(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.floor())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_ceil(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.ceil())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_round(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.round())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_trunc(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.trunc())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_max(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let mut max = std::f64::NEG_INFINITY;
    for arg in args {
        if let JsValue::Number(n) = arg {
            if n > max {
                max = n;
            }
        }
    }
    if max == std::f64::NEG_INFINITY {
        JsValue::Number(std::f64::NEG_INFINITY)
    } else {
        JsValue::Number(max)
    }
}

pub fn native_math_min(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let mut min = std::f64::INFINITY;
    for arg in args {
        if let JsValue::Number(n) = arg {
            if n < min {
                min = n;
            }
        }
    }
    if min == std::f64::INFINITY {
        JsValue::Number(std::f64::INFINITY)
    } else {
        JsValue::Number(min)
    }
}

pub fn native_math_pow(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Number(base)), Some(JsValue::Number(exp))) = (args.first(), args.get(1)) {
        JsValue::Number(base.powf(*exp))
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_sqrt(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.sqrt())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_cbrt(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.cbrt())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_random(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    JsValue::Number(fastrand::f64())
}

pub fn native_math_sin(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.sin())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_cos(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.cos())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_tan(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.tan())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_asin(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.asin())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_acos(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.acos())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_atan(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.atan())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_atan2(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Number(y)), Some(JsValue::Number(x))) = (args.first(), args.get(1)) {
        JsValue::Number(y.atan2(*x))
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_exp(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.exp())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_expm1(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.exp_m1())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_log(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.ln())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_log10(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.log10())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_log1p(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.ln_1p())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_log2(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.log2())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_sign(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.signum())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_hypot(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let mut sum_squares = 0.0;
    for arg in args {
        if let JsValue::Number(n) = arg {
            sum_squares += n * n;
        }
    }
    JsValue::Number(sum_squares.sqrt())
}

pub fn native_math_imul(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Number(a)), Some(JsValue::Number(b))) = (args.first(), args.get(1)) {
        let a = *a as i32;
        let b = *b as i32;
        JsValue::Number((a as i64 * b as i64) as f64)
    } else {
        JsValue::Number(0.0)
    }
}

pub fn native_math_fround(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        let result = (*n as f32) as f64;
        JsValue::Number(result)
    } else {
        JsValue::Number(0.0)
    }
}

pub fn native_math_clz32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        let n = *n as u32;
        JsValue::Number(n.leading_zeros() as f64)
    } else {
        JsValue::Number(32.0)
    }
}

pub fn native_math_sinh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.sinh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_cosh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.cosh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_tanh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.tanh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_asinh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.asinh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_acosh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.acosh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_math_atanh(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(n)) = args.first() {
        JsValue::Number(n.atanh())
    } else {
        JsValue::Number(std::f64::NAN)
    }
}
