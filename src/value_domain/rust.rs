use super::{Binding, Domain};
use std::fmt::Write;

pub(super) fn render(domain: &Domain, binding: &Binding) -> String {
    let Binding {
        type_name,
        constructor,
        accessor,
        maximum_constant,
        error_type,
        variants,
        ..
    } = binding;
    let integer = format!("::core::primitive::u{}", domain.bits);
    let error = &variants[&domain.range_error];
    let mut out = format!("#[derive(Debug, PartialEq, Eq)]\npub enum {error_type} {{\n");
    let order: Vec<_> = binding
        .variant_order
        .as_ref()
        .map_or_else(|| variants.keys().collect(), |ids| ids.iter().collect());
    for id in order {
        let variant = &variants[id];
        writeln!(out, "    {variant},").unwrap();
    }
    writeln!(out,"}}\nimpl ::core::fmt::Display for {error_type} {{\n    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{\n        f.write_str(match self {{").unwrap();
    for (id, variant) in variants {
        writeln!(
            out,
            "            Self::{variant} => {:?},",
            domain.errors[id]
        )
        .unwrap();
    }
    writeln!(
        out,
        "        }})\n    }}\n}}\nimpl ::core::error::Error for {error_type} {{}}"
    )
    .unwrap();
    let minimum = domain.minimum;
    let maximum = domain.maximum;
    let default = domain.default_value;
    writeln!(out,"#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct {type_name}(::core::option::Option<{integer}>);").unwrap();
    writeln!(out,"impl {type_name} {{
    pub const {maximum_constant}: {integer} = {maximum};

    pub fn {constructor}(value: ::core::option::Option<{integer}>) -> ::core::result::Result<Self, {error_type}> {{
        if value.is_some_and(|n| !({minimum}..=Self::{maximum_constant}).contains(&n)) {{
            return ::core::result::Result::Err({error_type}::{error});
        }}
        ::core::result::Result::Ok(Self(value))
    }}

    pub fn {accessor}(&self) -> ::core::option::Option<{integer}> {{
        self.0
    }}
}}
impl ::core::default::Default for {type_name} {{
    fn default() -> Self {{
        Self(::core::option::Option::Some({default}))
    }}
}}").unwrap();
    out
}
