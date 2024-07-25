use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, DeriveInput, Data, Fields};
use darling::FromField;

#[derive(FromField, Default)]
#[darling(default, attributes(option))]
struct FieldOpts {
    short: Option<String>,
    long: Option<String>,
    help: Option<String>,
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

        quote! {
            CliOption {
                name: stringify!(#field_name).to_string(),
                short: #short_opt,
                long: #long_opt,
                help: #help.to_string(),
                field_type_help: stringify!(#field_type).to_string().to_lowercase().replace(" ", ""),
                field_type: std::any::TypeId::of::<#field_type>(),
                required: !std::any::TypeId::of::<#field_type>().eq(&std::any::TypeId::of::<Option<()>>()),
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

    };

    TokenStream::from(expanded)
}