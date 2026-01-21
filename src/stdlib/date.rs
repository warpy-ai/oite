use crate::vm::value::{HeapData, HeapObject, JsValue};
use crate::vm::VM;
use chrono::{Datelike, Timelike};

pub fn native_date_constructor(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_millis() as f64;

    let timestamp = if args.is_empty() {
        now
    } else if let Some(JsValue::Number(n)) = args.first() {
        *n
    } else if let Some(JsValue::String(s)) = args.first() {
        parse_date_string(s)
    } else {
        now
    };

    let this_ptr = vm.call_stack.last().and_then(|frame| {
        if let JsValue::Object(ptr) = &frame.this_context {
            Some(*ptr)
        } else {
            None
        }
    });

    if let Some(ptr) = this_ptr {
        if let Some(heap_obj) = vm.heap.get_mut(ptr) {
            if let HeapData::Object(props) = &mut heap_obj.data {
                props.insert("_timestamp".to_string(), JsValue::Number(timestamp));
            }
        }
        // Return this object
        JsValue::Object(this_ptr.unwrap())
    } else {
        JsValue::Undefined
    }
}

pub fn native_date_now(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_millis() as f64;
    JsValue::Number(now)
}

pub fn native_date_parse(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(s)) = args.first() {
        JsValue::Number(parse_date_string(s))
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_date_utc(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let year = args
        .get(0)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as i32),
            _ => None,
        })
        .unwrap_or(1970);
    let month = args
        .get(1)
        .and_then(|v| match v {
            JsValue::Number(n) => Some((*n as i32 + 1) as u32),
            _ => None,
        })
        .unwrap_or(1);
    let day = args
        .get(2)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as u32),
            _ => None,
        })
        .unwrap_or(1);
    let hour = args
        .get(3)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as u32),
            _ => None,
        })
        .unwrap_or(0);
    let min = args
        .get(4)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as u32),
            _ => None,
        })
        .unwrap_or(0);
    let sec = args
        .get(5)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as u32),
            _ => None,
        })
        .unwrap_or(0);
    let ms = args
        .get(6)
        .and_then(|v| match v {
            JsValue::Number(n) => Some(*n as u32),
            _ => None,
        })
        .unwrap_or(0);

    if let Some(naive_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
        if let Some(naive_datetime) = naive_date.and_hms_milli_opt(hour, min, sec, ms) {
            return JsValue::Number(naive_datetime.timestamp_millis() as f64);
        }
    }
    JsValue::Number(std::f64::NAN)
}

pub fn native_date_get_time(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.timestamp_millis() as f64)
}

pub fn native_date_get_full_year(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.year() as f64)
}

pub fn native_date_get_month(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| (d.month() as i32 - 1) as f64)
}

pub fn native_date_get_date(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.day() as f64)
}

pub fn native_date_get_day(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.weekday().num_days_from_sunday() as f64)
}

pub fn native_date_get_hours(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.hour() as f64)
}

pub fn native_date_get_minutes(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.minute() as f64)
}

pub fn native_date_get_seconds(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.second() as f64)
}

pub fn native_date_get_milliseconds(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.timestamp_subsec_millis() as f64)
}

pub fn native_date_get_timezone_offset(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    JsValue::Number(0.0)
}

pub fn native_date_set_time(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Number(ms)) = args.first() {
        date_set_method(vm, &args, *ms as i64)
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

pub fn native_date_set_full_year(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 2024)
}

pub fn native_date_set_month(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 0)
}

pub fn native_date_set_date(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 1)
}

pub fn native_date_set_hours(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 0)
}

pub fn native_date_set_minutes(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 0)
}

pub fn native_date_set_seconds(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 0)
}

pub fn native_date_set_milliseconds(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_set_method(vm, &args, 0)
}

pub fn native_date_to_iso_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_to_string(vm, &args, DateFormat::ISO)
}

pub fn native_date_to_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_to_string(vm, &args, DateFormat::String)
}

pub fn native_date_to_utc_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_to_string(vm, &args, DateFormat::UTC)
}

pub fn native_date_value_of(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_get_method(vm, &args, |d| d.timestamp_millis() as f64)
}

pub fn native_date_to_json(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    date_to_string(vm, &args, DateFormat::ISO)
}

pub fn parse_date_string(s: &str) -> f64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S.%3fZ"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S"))
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(std::f64::NAN)
}

fn get_timestamp_from_args(vm: &VM, args: &[JsValue]) -> Option<f64> {
    let this_ptr = vm.call_stack.last().and_then(|frame| {
        if let JsValue::Object(ptr) = &frame.this_context {
            Some(*ptr)
        } else {
            None
        }
    });

    if let Some(ptr) = this_ptr {
        if let Some(heap_obj) = vm.heap.get(ptr) {
            if let HeapData::Object(props) = &heap_obj.data {
                if let Some(JsValue::Number(ts)) = props.get("_timestamp") {
                    return Some(*ts);
                }
            }
        }
    }

    None
}

enum DateFormat {
    ISO,
    String,
    UTC,
}

fn date_get_method<F>(vm: &VM, args: &[JsValue], f: F) -> JsValue
where
    F: FnOnce(chrono::NaiveDateTime) -> f64,
{
    let timestamp = get_timestamp_from_args(vm, args);
    let timestamp = match timestamp {
        Some(ts) => ts,
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO)
                .as_millis() as f64;
            now
        }
    };
    let secs = (timestamp as i64) / 1000;
    let nsecs = (((timestamp as i64) % 1000) * 1_000_000) as u32;
    if let Some(date) = chrono::NaiveDateTime::from_timestamp_opt(secs, nsecs) {
        JsValue::Number(f(date))
    } else {
        JsValue::Number(std::f64::NAN)
    }
}

fn date_set_method(vm: &mut VM, args: &[JsValue], _default: i64) -> JsValue {
    let timestamp = args
        .first()
        .and_then(|v| {
            if let JsValue::Number(n) = v {
                Some(*n as i64)
            } else {
                None
            }
        })
        .unwrap_or(_default);

    let this_ptr = vm.call_stack.last().and_then(|frame| {
        if let JsValue::Object(ptr) = &frame.this_context {
            Some(*ptr)
        } else {
            None
        }
    });

    if let Some(ptr) = this_ptr {
        if let Some(heap_obj) = vm.heap.get_mut(ptr) {
            if let HeapData::Object(props) = &mut heap_obj.data {
                props.insert("_timestamp".to_string(), JsValue::Number(timestamp as f64));
                return JsValue::Number(timestamp as f64);
            }
        }
    }

    JsValue::Number(std::f64::NAN)
}

fn date_to_string(vm: &VM, args: &[JsValue], format: DateFormat) -> JsValue {
    let timestamp = get_timestamp_from_args(vm, args);
    let timestamp = match timestamp {
        Some(ts) => ts,
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO)
                .as_millis() as f64;
            now
        }
    };
    let secs = (timestamp as i64) / 1000;
    let nsecs = (((timestamp as i64) % 1000) * 1_000_000) as u32;
    if let Some(date) = chrono::NaiveDateTime::from_timestamp_opt(secs, nsecs) {
        let s = match format {
            DateFormat::ISO => date.format("%Y-%m-%dT%H:%M:%S.%3fZ").to_string(),
            DateFormat::String => date.format("%a %b %d %Y %H:%M:%S").to_string(),
            DateFormat::UTC => date.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        };
        JsValue::String(s)
    } else {
        JsValue::String("Invalid Date".to_string())
    }
}
