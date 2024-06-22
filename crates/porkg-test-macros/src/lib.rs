use std::process::Termination;

use proc_macro::TokenStream;

mod fork;

#[proc_macro_attribute]
pub fn fork_test(attr: TokenStream, input: TokenStream) -> TokenStream {
    fork::fork_test(attr.into(), input.into()).into()
}
