use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};

pub(crate) fn fork_test(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut start = TokenStream::new();
    let mut ret = TokenStream::new();
    let mut iter = input.into_iter();

    for token in iter.by_ref() {
        match token {
            TokenTree::Ident(i) if i == "fn" => {
                start.extend([TokenTree::Ident(i)]);
                break;
            }
            other => start.extend([other]),
        }
    }

    let mut name = None;
    for token in iter.by_ref() {
        match token {
            TokenTree::Ident(i) => {
                name = Some(i.clone());
                start.extend([TokenTree::Ident(i)]);
                break;
            }
            other => start.extend([other]),
        }
    }

    let name = name.unwrap(); // TODO: Errors
    let mut in_ret = false;
    let mut iter = iter.peekable();
    loop {
        match (iter.next(), iter.peek()) {
            (Some(TokenTree::Group(g)), _) if g.delimiter() == Delimiter::Brace => {
                let ts = build_test(name, ret, g);
                let new_group = Group::new(Delimiter::Brace, ts);
                start.extend(quote::quote! { -> std::process::ExitCode });
                start.extend([TokenTree::Group(new_group)]);
                break;
            }
            (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                if a.as_char() == '-' && b.as_char() == '>' =>
            {
                iter.next();
                in_ret = true;
                start.extend(ret);
                ret = TokenStream::new();
            }
            (Some(other), _) if in_ret => ret.extend([other]),
            (Some(other), _) => start.extend([other]),
            _ => break,
        }
    }

    start
}

fn build_test(name: Ident, ret: TokenStream, g: proc_macro2::Group) -> TokenStream {
    let name = proc_macro2::Literal::string(&name.to_string());
    let g = g.stream();
    let g = if ret.is_empty() {
        quote::quote! {
            #g;
            std::process::ExitCode::SUCCESS
        }
    } else {
        quote::quote! {
            std::process::Termination::report({ #g } as #ret)
        }
    };

    quote::quote! {
        if porkg_test::fork::in_host() {
            #g
        } else {
           porkg_test::fork::run(module_path!(), #name)
        }
    }
}
