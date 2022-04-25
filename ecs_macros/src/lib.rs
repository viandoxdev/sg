#![feature(proc_macro_diagnostic)]
#![feature(proc_macro_span_shrink)]

use std::fmt::format;

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::spanned::Spanned;
use syn::{parse_macro_input, FnArg, ItemFn, PatType, Type};

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
        for arg in args {
            if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
                comps.push((*pat.to_owned(), *ty.to_owned()));
            }
        }

        let types = comps.iter().map(|(_, t)| {
            quote! {
                ::std::any::TypeId::of::<#t>()
            }
        });
        let lets = comps.iter().map(|(pat, ty)| quote!{
            let #pat = ((&mut **__components.get_mut(&::std::any::TypeId::of::<#ty>()).unwrap().get_mut(&__id).unwrap())
                    as &mut dyn ::std::any::Any).downcast_mut::<#ty>().unwrap();
        });
        let block = *fnc.block;

        return quote! {
            fn __pass(&mut self, __components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>) {
                let __reqs: ::std::collections::HashSet::<::std::any::TypeId> =
                    ::std::collections::HashSet::from_iter([#(#types),*].into_iter());
                let mut __comps = __components.iter()
                    .filter(|(k,_)| __reqs.contains(k))
                    .map(|(_, v)| v);
                let __uuids = __comps.next().expect("No required component list found").keys()
                    .filter(|k| __comps.all(|c| c.contains_key(k)))
                    .map(|u|  u.clone()).collect::<Vec<uuid::Uuid>>();
                for __id in __uuids {
                    #(#lets)*
                    #block;
                }
            }
            fn pass(
                components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>,
                systems: &mut ::std::collections::HashMap<::std::any::TypeId, Box<dyn ecs::System>>
            ) {
                let sys = systems.get_mut(&::std::any::TypeId::of::<Self>()).expect("System isn't part of ECS");
                let sys = ((&mut **sys) as &mut dyn ::std::any::Any).downcast_mut::<Self>().expect("Couldn't downcast system data struct");
                sys.__pass(components);
            }
        }.into();
    } else if &name_str == "pass_many" {
        // sig should be fn pass_many(&mut self, entities: Vec<(... components type)>) -> ()
        let mut comps = Vec::<syn::Type>::new();
        let entities_arg = args.next().expect("pass_many needs at least two arguments (fn pass_many(&mut self, entities: Vec<(...components...)>))");
        let entities_arg = match entities_arg {
            FnArg::Typed(ty) => ty,
            _ => unreachable!(),
        };
        let entities_pat = entities_arg.pat.to_owned();
        if let syn::Type::Path(syn::TypePath { path: syn::Path {segments, ..}, ..}) = &*entities_arg.ty {
            if let Some(syn::PathSegment {ident, arguments: syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {args, ..})}) = segments.iter().next() {
                if ident.to_string().as_str() == "Vec" {
                    if let Some(syn::GenericArgument::Type(syn::Type::Paren(syn::TypeParen {elem, ..}))) = args.iter().next() {
                        comps.push(*elem.to_owned());
                    } else if let Some(syn::GenericArgument::Type(syn::Type::Tuple(syn::TypeTuple {elems, ..}))) = args.iter().next() {
                        comps.extend(elems.iter().cloned());
                    } else {
                        entities_arg.span().unwrap().error("Second argument should be of form: name: Vec<(... components ...)>").emit();
                        return empty;
                    }
                    // If we get here, then the second argument is valid.
                    let types = comps.iter().map(|t| quote! {
                        ::std::any::TypeId::of::<#t>()
                    });
                    let comps = comps.iter().map(|t| quote!{
                        ((&mut **__map.get_mut(&::std::any::TypeId::of::<#t>()).unwrap().remove(&__id).unwrap()) as &mut dyn ::std::any::Any).downcast_mut::<#t>().unwrap()
                    });
                    let block = *fnc.block;

                    return quote! {
                        fn __pass(&mut self, __components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>) {
                            let __reqs: ::std::collections::HashSet::<::std::any::TypeId> =
                                ::std::collections::HashSet::from_iter([#(#types),*].into_iter());
                            let mut __comps = __components.iter()
                                .filter(|(k,_)| __reqs.contains(k))
                                .map(|(_, v)| v);
                            let __uuids = __comps.next().expect("No required component list found").keys()
                                .filter(|k| __comps.all(|c| c.contains_key(k)))
                                .map(|u|  u.clone()).collect::<Vec<uuid::Uuid>>();
                            let mut #entities_pat = Vec::new();
                            let mut __map = __components.iter_mut().map(|(k,v)|
                                    (*k, v.iter_mut().map(|(k, v)| (*k, v)).collect::<::std::collections::HashMap<uuid::Uuid, &mut Box<dyn ecs::Component + 'static>>>())
                                ).collect::<::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, &mut Box<dyn ecs::Component +'static>>>>();
                            for __id in __uuids {
                                #entities_pat.push(
                                    (
                                        #(#comps),*
                                    )
                                );
                            }
                            #block;
                        }
                        fn pass(
                            components: &mut ::std::collections::HashMap<::std::any::TypeId, ::std::collections::HashMap<uuid::Uuid, Box<dyn ecs::Component>>>,
                            systems: &mut ::std::collections::HashMap<::std::any::TypeId, Box<dyn ecs::System>>
                        ) {
                            let sys = systems.get_mut(&::std::any::TypeId::of::<Self>()).expect("System isn't part of ECS");
                            let sys = ((&mut **sys) as &mut dyn ::std::any::Any).downcast_mut::<Self>().expect("Couldn't downcast system data struct");
                            sys.__pass(components);
                        }
                    }.into()
                }
            }
        }
        entities_arg.span().unwrap().error("Second argument should be of form: name: Vec<(... components ...)>").emit();
    } else {
        name.span()
            .unwrap()
            .error("Function name should be either 'pass' 'pass_many'.")
            .emit();
    }

    quote! {}.into()
}
