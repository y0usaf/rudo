//! Derive macro for setting groups.
//!
//! This macro generates:
//! - a `{StructName}Changed` enum with one variant per field
//! - a minimal `SettingGroup` implementation that registers default values in `Settings`
//! - a `From<{StructName}Changed> for SettingsChanged` impl

use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DataStruct, DeriveInput, Error, Ident, parse_macro_input};

#[proc_macro_derive(SettingGroup, attributes(setting_prefix, option, alias))]
pub fn setting_group(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    stream(input)
}

fn stream(input: DeriveInput) -> TokenStream {
    const ERR_MSG: &str = "Derive macro expects a struct";
    match input.data {
        Data::Struct(ref data) => struct_stream(input.ident, data),
        Data::Enum(data) => Error::new_spanned(data.enum_token, ERR_MSG).to_compile_error().into(),
        Data::Union(data) => {
            Error::new_spanned(data.union_token, ERR_MSG).to_compile_error().into()
        }
    }
}

fn struct_stream(name: Ident, data: &DataStruct) -> TokenStream {
    let event_name = format_ident!("{}Changed", name);
    let name_without_settings = Ident::new(&name.to_string().replace("Settings", ""), name.span());

    let updated_case_fragments = data.fields.iter().map(|field| match field.ident.as_ref() {
        Some(field_ident) => {
            let case_name = field_ident.to_string().to_case(Case::Pascal);
            let case_ident = Ident::new(&case_name, field_ident.span());
            let ty = field.ty.clone();
            quote! {
                #case_ident(#ty),
            }
        }
        None => Error::new_spanned(field, "Expected named struct fields").to_compile_error(),
    });

    let expanded = quote! {
        #[derive(Debug, Clone, PartialEq, strum::AsRefStr)]
        pub enum #event_name {
            #(#updated_case_fragments)*
        }

        impl crate::settings::SettingGroup for #name {
            type ChangedEvent = #event_name;

            fn register(settings: &crate::settings::Settings) {
                let s: Self = Default::default();
                settings.set(&s);
            }
        }

        impl From<#event_name> for crate::settings::SettingsChanged {
            fn from(value: #event_name) -> Self {
                crate::settings::SettingsChanged::#name_without_settings(value)
            }
        }
    };

    TokenStream::from(expanded)
}
