use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::{Parse, ParseStream}, Token, LitStr, Result};
use syn::{parse_macro_input, DeriveInput, Data, Fields};
use darling::FromField;

#[derive(FromField, Default)]
#[darling(default, attributes(option))]
struct FieldOpts {
    short: Option<String>,
    long: Option<String>,
    help: Option<String>,
    required: Option<bool>,
    flag: Option<bool>,
    autocomplete: Option<syn::Path>,
}

#[derive(Debug)]
struct CommandInfo {    
    about: Option<String>,
    long_about: Option<String>,
    examples: Option<String>,
}

impl Parse for CommandInfo {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut info = CommandInfo {
            about: None,
            long_about: None,
            examples: None,
        };

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;

            match ident.to_string().as_str() {
                "about" => info.about = Some(value.value()),
                "long_about" => info.long_about = Some(value.value()),
                "examples" => info.examples = Some(value.value()),
                _ => return Err(input.error("Unknown field in command_info")),
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(info)
    }
}

#[proc_macro_derive(CliCommand, attributes(option, command_info))]
pub fn derive_cli_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let command_info = input.attrs.iter()
        .find(|attr| attr.path().is_ident("command_info"))
        .map(|attr| attr.parse_args::<CommandInfo>().expect("Failed to parse command_info"))
        .unwrap_or_else(|| CommandInfo { about: None, long_about: None, examples: None });

    let fields = match input.data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => fields,
                _ => panic!("CliCommand can only be derived for structs with named fields"),
            }
        },
        _ => panic!("CliCommand can only be derived for structs"),
    };

    let mut options: Vec<_> = fields.named.iter().map(|f| {
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

        let flag = opts.flag.map(|f| quote! { #f }).unwrap_or(quote! { false });

        let autocomplete_fn = opts.autocomplete.as_ref().map(|fn_path| {
            quote! { Some(#fn_path as fn(&crate::commandlist::CommandList, &str, &[String]) -> Vec<String>) }
        }).unwrap_or(quote! { None });

        quote! {
            CliOption {
                name: stringify!(#field_name).to_string(),
                short: #short_opt,
                long: #long_opt,
                help: #help.to_string(),
                field_type_help: stringify!(#field_type).to_string().to_lowercase().replace(" ", ""),
                field_type: std::any::TypeId::of::<#field_type>(),
                required: #required,
                flag: #flag,
                autocomplete: #autocomplete_fn,
            }
        }
    }).collect();

    options.push(quote! {
        CliOption {
            name: "help".to_string(),
            short: Some("-h".to_string()),
            long: Some("--help".to_string()),
            help: "Prints help information".to_string(),
            field_type_help: "bool".to_string(),
            field_type: std::any::TypeId::of::<bool>(),
            required: false,
            flag: true,
            autocomplete: None,
        }
    });

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
    
        let is_flag = opts.flag.unwrap_or(false);
    
        if is_flag {
            if is_optional {
                quote! {
                    if key == #short_opt || key == #long_opt {
                        obj.#field_name = Some(true);
                    }
                }
            } else {
                quote! {
                    if key == #short_opt || key == #long_opt {
                        obj.#field_name = true;
                    }
                }
            }
        } else {
            if is_optional {
                quote! {
                    if key == #short_opt || key == #long_opt {
                        obj.#field_name = Some(value.parse().map_err(|_| AppError::ParseError(format!("Option '{}' has value '{}' (expected type: {})", key, value, stringify!(#field_type).to_string().to_lowercase().replace(" ", ""))))?);
                    }
                }
            } else {
                quote! {
                    if key == #short_opt || key == #long_opt {
                        obj.#field_name = value.parse().map_err(|_| AppError::ParseError(format!("Option '{}' has value '{}' (expected type: {})", key, value, stringify!(#field_type).to_string().to_lowercase().replace(" ", ""))))?;
                    }
                }
            }
        }
    }).collect();
    
    let cmd_about = prepare_option_string(&command_info.about);
    let cmd_long_about = prepare_option_string(&command_info.long_about);
    let cmd_examples = prepare_option_string(&command_info.examples);

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
    
            fn about(&self) -> Option<String> {
                #cmd_about
            }

            fn long_about(&self) -> Option<String> {
                #cmd_long_about
            }

            fn examples(&self) -> Option<String> {
                #cmd_examples
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

// Helper function to prepare Option<String> values
fn prepare_option_string(opt: &Option<String>) -> proc_macro2::TokenStream {
    match opt {
        Some(s) => quote! { Some(#s.to_string()) },
        None => quote! { None },
    }
}