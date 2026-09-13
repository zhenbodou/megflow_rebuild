use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(Node)]
pub fn derive_node(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_node(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_node(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => return Err(syn::Error::new_spanned(input, "Node requires named fields")),
        },
        _ => return Err(syn::Error::new_spanned(input, "Node requires a struct")),
    };
    for required in ["output", "input_closed"] {
        if !fields
            .iter()
            .any(|field| field.ident.as_ref().is_some_and(|name| name == required))
        {
            return Err(syn::Error::new_spanned(
                input,
                format!("Node requires field {required}"),
            ));
        }
    }
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::flow_rs::node::Node for #name #type_generics #where_clause {
            fn close(&mut self) { self.output.close(); }
            fn is_all_input_closed(&self) -> bool { self.input_closed }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_field() {
        let input: DeriveInput = syn::parse_quote!(
            struct Missing {
                output: Sender,
            }
        );
        assert!(expand_node(&input)
            .unwrap_err()
            .to_string()
            .contains("input_closed"));
    }

    #[test]
    fn preserves_generics_in_generated_impl() {
        let input: DeriveInput = syn::parse_quote!(
            struct Example<T>
            where
                T: Clone,
            {
                output: Sender,
                input_closed: bool,
                value: T,
            }
        );
        let generated = expand_node(&input).unwrap();
        let implementation: syn::ItemImpl = syn::parse2(generated).unwrap();
        assert_eq!(implementation.generics.params.len(), 1);
        assert!(implementation.generics.where_clause.is_some());
    }
}
