use std::collections::BTreeSet;

use quote::ToTokens;
use serde_derive_internals::ast::{Container, Data, Field, Style, Variant};
use serde_derive_internals::attr::TagType;
use serde_derive_internals::{Ctxt, Derive};
use syn::{Attribute, GenericArgument, Path, PathArguments, Type};

use super::source::{Definition, Source};

pub(super) fn name(key: &str) -> &str {
    match key {
        "crate::reply::Hand" => "ReplyHand",
        "crate::chat::Line" => "LineKind",
        "crate::lookback::Read" => "LookbackRead",
        other => other.rsplit("::").next().unwrap_or(other),
    }
}

fn guard(attrs: &[Attribute]) -> syn::Result<()> {
    if let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr")) {
        return Err(syn::Error::new_spanned(attr, "conditional wire declarations are unsupported"));
    }
    Ok(())
}

fn key(raw: &str) -> String {
    if raw.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        raw.to_owned()
    } else {
        serde_json::Value::from(raw).to_string()
    }
}

pub(super) struct Render<'a> {
    pub source: &'a Source,
    pub pending: &'a mut BTreeSet<String>,
    pub module: &'a str,
}

fn type_args(path: &Path) -> syn::Result<Vec<&Type>> {
    let mut segments = path.segments.iter().rev();
    let last = segments.next().ok_or_else(|| syn::Error::new_spanned(path, "empty type path"))?;
    if let Some(s) = segments.find(|s| !s.arguments.is_empty()) {
        return Err(syn::Error::new_spanned(s, "unsupported associated wire type"));
    }
    match &last.arguments {
        PathArguments::None => Ok(Vec::new()),
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .map(|arg| match arg {
                GenericArgument::Type(ty) => Ok(ty),
                _ => Err(syn::Error::new_spanned(arg, "unsupported wire type argument")),
            })
            .collect(),
        args => Err(syn::Error::new_spanned(args, "unsupported wire type arguments")),
    }
}

impl Render<'_> {
    pub fn declaration(&mut self, def: &Definition) -> syn::Result<String> {
        let input = &def.input;
        guard(&input.attrs)?;
        let derives_serialize = input.attrs.iter().filter(|a| a.path().is_ident("derive")).any(|a| {
            a.parse_args_with(syn::punctuated::Punctuated::<Path, syn::Token![,]>::parse_terminated)
                .is_ok_and(|paths| paths.iter().any(|p| p.segments.last().is_some_and(|s| s.ident == "Serialize")))
        });
        if !derives_serialize || !input.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(&input.ident, "expected a concrete type with derived Serialize"));
        }
        let cx = Ctxt::new();
        let container = Container::from_ast(&cx, input, Derive::Serialize);
        cx.check()?;
        let c = container.ok_or_else(|| syn::Error::new_spanned(input, "unsupported wire declaration"))?;
        if let Some(ty) = c.attrs.type_into() {
            return self.ty(ty);
        }
        if c.attrs.transparent() {
            let f = c
                .data
                .all_fields()
                .find(|f| f.attrs.transparent())
                .ok_or_else(|| syn::Error::new_spanned(input, "missing transparent field"))?;
            return self.field_type(f);
        }
        match &c.data {
            Data::Struct(style, fields) if matches!(c.attrs.tag(), TagType::External) => self.fields(*style, fields),
            Data::Enum(variants) => {
                let mut items = Vec::new();
                for v in variants {
                    guard(&v.original.attrs)?;
                    if !v.attrs.skip_serializing() {
                        items.push(self.variant(c.attrs.tag(), v)?.replace('\n', "\n    "));
                    }
                }
                Ok(if items.is_empty() { "never".to_owned() } else { format!("\n  | {}", items.join("\n  | ")) })
            }
            _ => Err(syn::Error::new_spanned(input, "unsupported tagged wire struct")),
        }
    }

    fn variant(&mut self, tag: &TagType, v: &Variant<'_>) -> syn::Result<String> {
        if v.attrs.serialize_with().is_some() {
            return Err(syn::Error::new_spanned(v.original, "unsupported custom variant serializer"));
        }
        let raw = v.attrs.name().serialize_name();
        let literal = serde_json::Value::from(raw).to_string();
        let tag = if v.attrs.untagged() { &TagType::None } else { tag };
        if matches!(v.style, Style::Unit) && matches!(tag, TagType::External) {
            return Ok(literal);
        }
        let body = self.fields(v.style, &v.fields)?;
        match tag {
            TagType::External => Ok(format!("{{ {}: {body} }}", key(raw))),
            TagType::None => Ok(body),
            TagType::Internal { tag } => {
                let tagged = format!("{{ {}: {literal} }}", key(tag));
                Ok(if matches!(v.style, Style::Unit) || body == "Record<string, never>" {
                    tagged
                } else {
                    format!("{tagged} & {body}")
                })
            }
            TagType::Adjacent { .. } => Err(syn::Error::new_spanned(v.original, "unsupported adjacent wire tags")),
        }
    }

    fn fields(&mut self, style: Style, fields: &[Field<'_>]) -> syn::Result<String> {
        match style {
            Style::Unit => Ok("null".to_owned()),
            Style::Struct => self.object(fields),
            Style::Newtype if let [field] = fields => self.field_type(field),
            _ => Err(syn::Error::new_spanned(fields.first().map(|f| f.original), "unsupported tuple wire declaration")),
        }
    }

    fn object(&mut self, fields: &[Field<'_>]) -> syn::Result<String> {
        let mut lines = Vec::new();
        let mut parts = Vec::new();
        for f in fields {
            guard(&f.original.attrs)?;
            if f.attrs.skip_serializing() {
                continue;
            }
            let ty = self.field_type(f)?;
            let skip = f.attrs.skip_serializing_if();
            if f.attrs.flatten() {
                if skip.is_some() {
                    return Err(syn::Error::new_spanned(f.original, "unsupported optional flattened field"));
                }
                parts.push(ty);
            } else {
                let field = key(f.attrs.name().serialize_name());
                let optional = if skip.is_some() { "?" } else { "" };
                lines.push(format!("  {field}{optional}: {ty};"));
            }
        }
        if !lines.is_empty() {
            parts.insert(0, format!("{{\n{}\n}}", lines.join("\n")));
        }
        Ok(if parts.is_empty() { "Record<string, never>".to_owned() } else { parts.join(" & ") })
    }

    fn field_type(&mut self, f: &Field<'_>) -> syn::Result<String> {
        guard(&f.original.attrs)?;
        if f.attrs.serialize_with().is_some() {
            return Err(syn::Error::new_spanned(f.original, "unsupported custom field serializer"));
        }
        let ty = self.ty(f.ty)?;
        let Some(predicate) = f.attrs.skip_serializing_if() else { return Ok(ty) };
        let path = predicate.path.to_token_stream().to_string().replace(' ', "");
        if matches!(path.as_str(), "Option::is_none" | "std::option::Option::is_none" | "core::option::Option::is_none")
        {
            return ty
                .strip_suffix(" | null")
                .map(str::to_owned)
                .ok_or_else(|| syn::Error::new_spanned(f.original, "expected Option for Option::is_none"));
        }
        if ty.ends_with(" | null") {
            return Err(syn::Error::new_spanned(f.original, "unsupported omission predicate for nullable field"));
        }
        Ok(ty)
    }

    fn ty(&mut self, ty: &Type) -> syn::Result<String> {
        match ty {
            Type::Path(p) if p.qself.is_none() => self.path(&p.path),
            Type::Tuple(t) if t.elems.is_empty() => Ok("null".to_owned()),
            Type::Tuple(t) => {
                let items = t.elems.iter().map(|ty| self.ty(ty)).collect::<Result<Vec<_>, _>>()?;
                Ok(format!("[{}]", items.join(", ")))
            }
            _ => Err(syn::Error::new_spanned(ty, "unsupported wire type")),
        }
    }

    fn path(&mut self, path: &Path) -> syn::Result<String> {
        let resolved = self.source.resolve(self.module, path).map_err(|e| syn::Error::new_spanned(path, e))?;
        let args = type_args(path)?;
        match (resolved.as_str(), args.as_slice()) {
            ("String" | "std::string::String", []) => Ok("string".to_owned()),
            ("bool", []) => Ok("boolean".to_owned()),
            ("u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" | "f32" | "f64", []) => {
                Ok("number".to_owned())
            }
            ("serde_json::Value", []) => Ok("unknown".to_owned()),
            ("Vec" | "std::vec::Vec", [inner]) => Ok(format!("Array<{}>", self.ty(inner)?)),
            ("Option" | "std::option::Option", [inner]) => Ok(format!("{} | null", self.ty(inner)?)),
            ("Box" | "std::boxed::Box", [inner]) => self.ty(inner),
            ("std::collections::BTreeMap", [k, v]) if self.ty(k)? == "string" => {
                Ok(format!("Record<string, {}>", self.ty(v)?))
            }
            (local, []) if self.source.definitions.contains_key(local) => {
                self.pending.insert(local.to_owned());
                Ok(name(local).to_owned())
            }
            _ => Err(syn::Error::new_spanned(path, format!("unsupported wire type {resolved}"))),
        }
    }
}
