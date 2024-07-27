use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Data, Fields};
use darling::FromField;

#[derive(FromField, Default)]
#[darling(default, attributes(option))]
struct FieldOpts {
    short: Option<String>,
    long: Option<String>,
    help: Option<String>,
    required: Option<bool>,
}

#[proc_macro_derive(CliCommand, attributes(option))]
pub fn derive_cli_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match input.data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => fields,
                _ => panic!("CliCommand can only be derived for structs with named fields"),
            }
        },
        _ => panic!("CliCommand can only be derived for structs"),
    };

    let options: Vec<_> = fields.named.iter().map(|f| {
        let opts = FieldOpts::from_field(f).unwrap_or_default();
        let field_name = f.ident.as_ref().unwrap();
        let field_type = &f.ty;
        
        let short_opt = opts.short.map(|c| quote! { Some(format!("-{}", #c)) }).unwrap_or(quote! { None });
        let long_opt = opts.long.as_ref().map(
            |field_name| quote! { Some(format!("--{}", #field_name)) },
        ).unwrap_or(quote! { None });
        let help = opts.help.as_ref().map(|h| quote! { #h }).unwrap_or(quote! { String::new() });

        let is_optional = match field_type {
            syn::Type::Path(type_path) => {
                type_path.path.segments.last()
                    .map(|seg| seg.ident == "Option")
                    .unwrap_or(false)
            },
            _ => false,
        };

        let required = if is_optional {
            quote! { false }
        } else {
            opts.required.map(|r| quote! { #r }).unwrap_or(quote! { true })
        };

        quote! {
            CliOption {
                name: stringify!(#field_name).to_string(),
                short: #short_opt,
                long: #long_opt,
                help: #help.to_string(),
                field_type_help: stringify!(#field_type).to_string().to_lowercase().replace(" ", ""),
                field_type: std::any::TypeId::of::<#field_type>(),
                required: #required,
            }
        }
    }).collect();

    let field_setters: Vec<_> = fields.named.iter().map(|f| {
        let field_name = f.ident.as_ref().unwrap();
        let field_type = &f.ty;
        let opts = FieldOpts::from_field(f).unwrap_or_default();
        let short_opt = opts.short.as_ref().map(|s| s.to_string());
        let long_opt = opts.long.as_ref().map(|l| l.to_string());
        
        let is_optional = match &f.ty {
            syn::Type::Path(type_path) => {
                type_path.path.segments.last()
                    .map(|seg| seg.ident == "Option")
                    .unwrap_or(false)
            },
            _ => false,
        };                    

        if is_optional {
            quote! {
                if key == #short_opt || key == #long_opt {
                    obj.#field_name = Some(value.parse().map_err(|_| AppError::ParseError(format!("Option '{}' has value {} (expected type: {})", key, value, stringify!(#field_type).to_string().to_lowercase().replace(" ", "")).to_string()))?);
                }
            }
        } else {
            quote! {
                if key == #short_opt || key == #long_opt {
                    obj.#field_name = value.parse().map_err(|_| AppError::ParseError(format!("Option '{}' has value {} (expected type: {})", key, value, stringify!(#field_type).to_string().to_lowercase().replace(" ", "")).to_string()))?;
                }
            }
        }
    }).collect();

    let expanded = quote! {
        impl CliCommandInfo for #name {
            fn options(&self) -> Vec<CliOption> {
                vec![
                    #(#options),*
                ]
            }
            fn name(&self) -> String {
                stringify!(#name).to_string()
            }

        }

        impl #name {
            pub fn new_from_tokens(&self, tokens: &CommandTokenizer) -> Result<Self, AppError> {
                let mut obj = Self::default();
                obj.validate(tokens)?;

                for (key, value) in tokens.get_options() {
                    #(#field_setters)*
                }

                Ok(obj)
            }
        }

    };

    TokenStream::from(expanded)
}