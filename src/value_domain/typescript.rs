use super::Domain;

pub(super) fn render(domain: &Domain) -> String {
    let minimum = domain.minimum;
    let maximum = domain.maximum;
    let default = domain.default_value;
    let error = serde_json::to_string(&domain.range_error).expect("string serialization");
    let messages = serde_json::to_string(&domain.errors).expect("string map serialization");
    format!(
        "export const maximum = {maximum}n;
export const defaultValue = {default}n;
export const errorMessages = {messages} as const;

export type Result =
    | {{ ok: true; value: bigint | null }}
    | {{ ok: false; error: typeof rangeError }};

const rangeError = {error} as const;

export function validate(value: bigint | null): Result {{
    if (value === null || (typeof value === 'bigint' && value >= {minimum}n && value <= maximum)) {{
        return {{ ok: true, value }};
    }}
    return {{ ok: false, error: rangeError }};
}}
"
    )
}
