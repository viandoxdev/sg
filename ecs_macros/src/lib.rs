#![feature(proc_macro_diagnostic)]
#![feature(proc_macro_span_shrink)]

use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{parse_macro_input, FnArg, ItemFn, PatType};

/// See if ty is of form Vec<...> and return Some(...) if it is.
fn parse_vec(ty: syn::Type) -> Option<syn::Type> {
    if let syn::Type::Path(syn::TypePath {
        path: syn::Path { segments, .. },
        ..
    }) = ty
    {
        if let syn::PathSegment {
            ident,
            arguments:
                syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }),
        } = segments.into_iter().last()?
        {
            if ident.to_string() == "Vec" {
                if let syn::GenericArgument::Type(out) = args.into_iter().next()? {
                    return Some(out);
                }
            }
        }
    }
    None
}

/// See if ty is of form Vec<...> and return Some(...) if it is.
fn parse_hashmap(ty: syn::Type) -> Option<(syn::Type, syn::Type)> {
    if let syn::Type::Path(syn::TypePath {
        path: syn::Path { segments, .. },
        ..
    }) = ty.to_owned()
    {
        if let syn::PathSegment {
            ident,
            arguments:
                syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }),
        } = segments.into_iter().last()?
        {
            if ident.to_string() == "HashMap" {
                let mut args = args.into_iter().filter_map(|arg| match arg {
                    syn::GenericArgument::Type(ty) => Some(ty),
                    _ => None,
                });
                return Some((args.next()?, args.next()?));
            }
        }
    }
    None
}

/// See if ty is of form Option<...> and return Some(...) if it is.
fn parse_option(ty: syn::Type) -> Option<syn::Type> {
    if let syn::Type::Path(syn::TypePath {
        path: syn::Path { segments, .. },
        ..
    }) = ty
    {
        if let syn::PathSegment {
            ident,
            arguments:
                syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }),
        } = segments.into_iter().last()?
        {
            if ident.to_string() == "Option" {
                if let syn::GenericArgument::Type(out) = args.into_iter().next()? {
                    return Some(out);
                }
            }
        }
    }
    None
}

fn get_generics(ty: syn::Type) -> Vec<syn::Type> {
    if let syn::Type::Path(syn::TypePath {
        path: syn::Path { segments, .. },
        ..
    }) = ty
    {
        if let Some(syn::PathSegment {
            ident,
            arguments:
                syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments { args, .. }),
        }) = segments.into_iter().last()
        {
            if ident.to_string() == "Option" {
                return args
                    .into_iter()
                    .filter_map(|arg| match arg {
                        syn::GenericArgument::Type(ty) => Some(ty),
                        _ => None,
                    })
                    .collect();
            }
        }
    }
    vec![]
}

/// Split type to vec:
/// A -> vec![A]
/// (A) -> vec![A]
/// (A, B, ...) -> vec![A, B, ...]
fn split_type(ty: syn::Type) -> Vec<syn::Type> {
    if let syn::Type::Paren(syn::TypeParen { elem, .. }) = ty {
        vec![*elem.to_owned()]
    } else if let syn::Type::Tuple(syn::TypeTuple { elems, .. }) = ty {
        elems.into_iter().collect::<Vec<_>>()
    } else {
        vec![ty]
    }
}

#[proc_macro_attribute]
pub fn system_pass(_: TokenStream, item: TokenStream) -> TokenStream {
    let empty = quote!().into();
    let fnc: ItemFn = parse_macro_input!(item);
    let name = fnc.sig.ident.clone();
    let name_str = name.to_string();
    let mut args = fnc.sig.inputs.iter();
    let first_arg = args.next();

    // check if first arg is &mut self
    if let Some(FnArg::Receiver(syn::Receiver {
        reference: Some((syn::token::And { .. }, None)),
        mutability: Some(syn::token::Mut { .. }),
        ..
    })) = first_arg
    {
    } else {
        let span = first_arg.map_or_else(|| fnc.span().unwrap(), |a| a.span().unwrap());
        span.error("First argument should be &mut self").emit();
        return empty;
    }

    if &name_str == "pass" {
        // sig should be fn pass(&mut self, ... components) -> ()
        let mut comps = Vec::<(syn::Pat, syn::Type)>::new();
        let mut comps_opt = Vec::<(syn::Pat, syn::Type)>::new();

        for arg in args {
            if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
                match parse_option(*ty.to_owned()) {
                    Some(ty) => comps_opt.push((*pat.to_owned(), ty)),
                    None => comps.push((*pat.to_owned(), *ty.to_owned())),
                }
            }
        }

        let reqs = comps
            .iter()
            .map(|(_, ty)| {
                quote! {
                    .add::<#ty>()
                }
            })
            .chain(comps_opt.iter().map(|(_, ty)| {
                quote! {
                    .add_optional::<#ty>()
                }
            }));
        let lets = comps.iter().map(|(pat, ty)| quote!{
            let #pat = ecs::downcast_component::<#ty>(&mut __entity).expect("Missing requried component on filtered entity.");
        }).chain(comps_opt.iter().map(|(pat, ty)| quote!{
            let #pat = ecs::downcast_component::<#ty>(&mut __entity);
        }));
        let block = *fnc.block;

        return quote! {
            fn pass(&mut self, __components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>) {
                let __reqs = ecs::SystemRequirements::new()
                #(#reqs)*;
                let __entities = __reqs.filter(__components);
                for (__id, mut __entity) in __entities {
                    #(#lets)*
                    #block;
                }
            }
        }.into();
    } else if &name_str == "pass_many" {
        // sig should be fn pass_many(&mut self, entities: Vec<(... components type)>) -> ()
        let entities_arg = args.next().expect("pass_many needs at least two arguments (fn pass_many(&mut self, entities: Vec<(...components...)>))");
        let entities_arg = match entities_arg {
            FnArg::Typed(ty) => ty,
            _ => unreachable!(),
        };
        let entities_pat = *entities_arg.pat.to_owned();
        let entities_type = *entities_arg.ty.to_owned();

        if let Some((_, ty)) = parse_hashmap(entities_type) {
            let comps = split_type(ty)
                .into_iter()
                .map(|ty| match parse_option(ty.to_owned()) {
                    Some(ty) => (ty, true),
                    None => (ty, false),
                })
                .collect::<Vec<(syn::Type, bool)>>();
            let reqs = comps.iter().map(|(ty, opt)| {
                let name = if *opt {
                    quote! {add_optional}
                } else {
                    quote! {add}
                };
                quote! {
                    .#name::<#ty>()
                }
            });
            let tuple = comps.iter().map(|(ty, opt)| {
                let unwrap = if *opt {
                    quote! {}
                } else {
                    quote! {.unwrap()}
                };
                quote! {
                    ecs::downcast_component::<#ty>(&mut e)#unwrap
                }
            });
            let block = *fnc.block;

            return quote! {
                fn pass(&mut self, __components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>) {
                    let __reqs = ecs::SystemRequirements::new()
                        #(#reqs)*;
                    let mut __entities = __reqs.filter(__components);
                    let #entities_pat = __entities.into_iter().map(|(id, mut e)| (id, (#(#tuple),*))).collect::<::std::collections::HashMap<_, _>>();
                    #block;
                }
            }.into();
        } else {
            entities_arg
                .span()
                .unwrap()
                .error("Second argument should be of form: name: Vec<(... components ...)>")
                .emit();
        }
    } else {
        name.span()
            .unwrap()
            .error("Function name should be either 'pass' 'pass_many'.")
            .emit();
    }

    quote! {}.into()
}
