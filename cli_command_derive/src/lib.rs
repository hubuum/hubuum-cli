use proc_macro::TokenStream;
use quote::quote;
use darling::FromField;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[derive(FromField, Default)]
#[darling(default, attributes(option))]
struct FieldOpts {
    short: Option<String>,
    long: Option<String>,
    help: Option<String>,
    required: Option<bool>,
    flag: Option<bool>,
    greedy: Option<bool>,
    nargs: Option<usize>,
    autocomplete: Option<syn::Path>,
}

#[proc_macro_derive(CommandArgs, attributes(option))]
pub fn derive_command_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match input.data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => fields,
                _ => panic!("CommandArgs can only be derived for structs with named fields"),
            }
        },
        _ => panic!("CommandArgs can only be derived for structs"),
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

        let is_optional = is_outer_type(field_type, "Option");
        let is_vec = is_outer_type(field_type, "Vec");

        let required = if is_optional || is_vec {
            quote! { false }
        } else {
            opts.required.map(|r| quote! { #r }).unwrap_or(quote! { true })
        };
        let repeatable = if is_vec {
            quote! { true }
        } else {
            quote! { false }
        };

        let flag = opts.flag.map(|f| quote! { #f }).unwrap_or(quote! { false });
        let greedy = opts.greedy.map(|g| quote! { #g }).unwrap_or(quote! { false });
        let nargs = opts.nargs.map(|n| quote! { Some(#n) }).unwrap_or(quote! { None });

        let autocomplete_fn = opts.autocomplete.as_ref().map(|fn_path| {
            quote! { Some(#fn_path as fn(&crate::services::CompletionContext, &str, &[String]) -> Vec<String>) }
        }).unwrap_or(quote! { None });

        quote! {
            crate::commands::CliOption {
                name: stringify!(#field_name).to_string(),
                short: #short_opt,
                long: #long_opt,
                help: #help.to_string(),
                field_type_help: stringify!(#field_type).to_string().to_lowercase().replace(" ", ""),
                field_type: std::any::TypeId::of::<#field_type>(),
                required: #required,
                flag: #flag,
                greedy: #greedy,
                nargs: #nargs,
                repeatable: #repeatable,
                autocomplete: #autocomplete_fn,
            }
        }
    }).collect();


    let field_setters: Vec<_> = fields.named.iter().map(|f| {
        let opts       = FieldOpts::from_field(f).unwrap_or_default();
        let field_name = f.ident.as_ref().unwrap();
        let field_type = &f.ty;

        // are we an Option<T>?
        let is_optional = is_outer_type(field_type, "Option");
        let is_vec = is_outer_type(field_type, "Vec");
        let is_flag = opts.flag.unwrap_or(false);
        let inner_vec_type = vec_inner_type(field_type);
        let inner_option_type = option_inner_type(field_type);

        // Use the *stripped* names here, exactly as the tokenizer stores them.
        //   opts.short = Some("f"), opts.long = Some("foo")
        let short_str = opts.short.clone();
        let long_str  = opts.long.clone();

        // Build matcher on *those* strings:
        let matcher = match (short_str, long_str) {
            (Some(short), Some(long)) => {
                quote! { key == #short || key == #long }
            }
            (Some(short), None) => {
                quote! { key == #short }
            }
            (None, Some(long)) => {
                quote! { key == #long }
            }
            (None, None) => panic!(
                "CommandArgs derive: field `{}` has neither short nor long!",
                stringify!(#field_name)
            ),
        };

        if is_vec {
            let inner_type = inner_vec_type.expect("vec type should have inner type");
            if is_flag {
                panic!(
                    "CommandArgs derive: Vec fields cannot be declared as flags: `{}`",
                    stringify!(#field_name)
                );
            }

            let parse_value = quote! {
                value.parse::<#inner_type>().map_err(|_| crate::errors::AppError::ParseError(
                    format!(
                        "Option '{}' has value '{}' (expected type: {})",
                        key, value,
                        stringify!(#inner_type).to_lowercase()
                    )
                ))?
            };

            quote! {
                {
                    let mut values = Vec::new();
                    for occurrence in tokens.get_option_occurrences() {
                        let key = occurrence.key.as_str();
                        let value = occurrence.value.as_str();
                        if #matcher {
                            values.push(#parse_value);
                        }
                    }
                    obj.#field_name = values;
                }
            }
        } else if is_flag {
            // boolean / flag field
            if is_optional {
                // e.g. Option<bool>
                quote! {
                    if #matcher {
                        obj.#field_name = Some(true);
                    }
                }
            } else {
                // e.g. plain bool
                quote! {
                    if #matcher {
                        obj.#field_name = true;
                    }
                }
            }
        } else if is_optional {
            // Option<T> with a value
            let inner_type = inner_option_type.expect("option type should have inner type");
                quote! {
                    if #matcher {
                        obj.#field_name = Some(
                            value.parse::<#inner_type>().map_err(|_| crate::errors::AppError::ParseError(
                                format!(
                                    "Option '{}' has value '{}' (expected type: {})",
                                    key, value,
                                stringify!(#inner_type).to_lowercase()
                            )
                        ))?
                    );
                }
            }
        } else {
            // T with a value
                quote! {
                    if #matcher {
                        obj.#field_name = value.parse().map_err(|_| crate::errors::AppError::ParseError(
                            format!(
                                "Option '{}' has value '{}' (expected type: {})",
                                key, value,
                            stringify!(#field_type).to_lowercase()
                        )
                    ))?;
                }
            }
        }
    }).collect();
    let expanded = quote! {
        impl crate::commands::CommandArgs for #name {
            fn options() -> Vec<crate::commands::CliOption> {
                vec![
                    #(#options),*
                ]
            }

            fn parse_tokens(tokens: &crate::tokenizer::CommandTokenizer) -> Result<Self, crate::errors::AppError> {
                let mut obj = Self::default();
                crate::commands::validate_command_args::<Self>(tokens)?;

                for (key, value) in tokens.get_options() {
                    #(#field_setters)*
                }

                Ok(obj)
            }
        }

        impl #name {
            pub fn parse_tokens(tokens: &crate::tokenizer::CommandTokenizer) -> Result<Self, crate::errors::AppError> {
                <Self as crate::commands::CommandArgs>::parse_tokens(tokens)
            }
        }
    };

    TokenStream::from(expanded)
}

fn is_outer_type(field_type: &syn::Type, expected: &str) -> bool {
    match field_type {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident == expected)
            .unwrap_or(false),
        _ => false,
    }
}

fn option_inner_type(field_type: &syn::Type) -> Option<&syn::Type> {
    path_inner_type(field_type, "Option")
}

fn vec_inner_type(field_type: &syn::Type) -> Option<&syn::Type> {
    path_inner_type(field_type, "Vec")
}

fn path_inner_type<'a>(field_type: &'a syn::Type, expected: &str) -> Option<&'a syn::Type> {
    let syn::Type::Path(type_path) = field_type else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != expected {
        return None;
    }

    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|arg| {
        if let syn::GenericArgument::Type(inner_type) = arg {
            Some(inner_type)
        } else {
            None
        }
    })
}
