use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use serde_json::{Number, Value};

/// Finite JSON numbers with exact integer identity and ordering.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NumericValue(Repr);

#[derive(Debug, Clone, Copy)]
enum Repr {
    Integer(i128),
    Float(f64),
}

impl NumericValue {
    pub(crate) fn from_number(number: &Number) -> Self {
        if let Some(value) = number.as_i64() {
            return Self(Repr::Integer(value.into()));
        }
        if let Some(value) = number.as_u64() {
            return Self(Repr::Integer(value.into()));
        }
        let value = number.as_f64().expect("JSON numbers are finite");
        if value.fract() == 0.0
            && (-9_223_372_036_854_775_808.0..18_446_744_073_709_551_616.0).contains(&value)
        {
            Self(Repr::Integer(value as i128))
        } else {
            Self(Repr::Float(value))
        }
    }

    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        match value {
            Value::Number(number) => Ok(Self::from_number(number)),
            Value::String(text) => serde_json::from_str::<Number>(text)
                .map(|number| Self::from_number(&number))
                .map_err(|_| "expected one complete finite JSON number".to_string()),
            _ => Err("expected one complete finite JSON number".to_string()),
        }
    }

    pub(crate) fn as_integer(self) -> Option<i128> {
        match self.0 {
            Repr::Integer(value) => Some(value),
            Repr::Float(_) => None,
        }
    }

    pub(crate) fn as_f64(self) -> f64 {
        match self.0 {
            Repr::Integer(value) => value as f64,
            Repr::Float(value) => value,
        }
    }
}

fn compare_integer_float(integer: i128, float: f64) -> Ordering {
    if float >= 18_446_744_073_709_551_616.0 {
        return Ordering::Less;
    }
    if float < -9_223_372_036_854_775_808.0 {
        return Ordering::Greater;
    }
    integer
        .cmp(&(float as i128))
        .then_with(|| 0.0_f64.partial_cmp(&float.fract()).expect("finite float"))
}

impl Ord for NumericValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.0, other.0) {
            (Repr::Integer(left), Repr::Integer(right)) => left.cmp(&right),
            (Repr::Float(left), Repr::Float(right)) => {
                left.partial_cmp(&right).expect("finite floats")
            }
            (Repr::Integer(left), Repr::Float(right)) => compare_integer_float(left, right),
            (Repr::Float(left), Repr::Integer(right)) => {
                compare_integer_float(right, left).reverse()
            }
        }
    }
}

impl PartialOrd for NumericValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for NumericValue {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for NumericValue {}

impl Hash for NumericValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.0 {
            Repr::Integer(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Repr::Float(value) => {
                1_u8.hash(state);
                value.to_bits().hash(state);
            }
        }
    }
}

pub(crate) fn sum_numbers<'a>(values: impl Iterator<Item = &'a Number>) -> Value {
    let mut integer = Some(0_i128);
    let mut float = 0.0;
    let mut any = false;
    for value in values {
        any = true;
        let number = NumericValue::from_number(value);
        float += number.as_f64();
        integer = integer
            .zip((!value.is_f64()).then(|| number.as_integer()).flatten())
            .and_then(|(a, b)| a.checked_add(b));
    }
    if !any {
        return Value::Null;
    }
    integer
        .and_then(|value| {
            i64::try_from(value)
                .map(Value::from)
                .or_else(|_| u64::try_from(value).map(Value::from))
                .ok()
        })
        .unwrap_or_else(|| Value::from(float))
}
