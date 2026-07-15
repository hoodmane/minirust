//@ compile-flags: --minirust-tree-borrows --minirust-tree-borrows-implicit-writes
#![allow(internal_features)]
#![feature(rustc_attrs)]

// Same code as `ub/tree_borrows/implicit_writes/as_mut_ptr.rs`, but with
// `#[rustc_no_writable]` on `as_mut_ptr`. This disables implicit writes for
// that function, so no UB occurs.

fn main() {
    let mut x: [u8; 3] = [1, 2, 3];

    let ptr = std::ptr::from_mut(&mut x);
    let a = unsafe { &mut *ptr };
    let b = unsafe { &mut *ptr };

    let _c = as_mut_ptr(a);
    let _v = *b;
}

#[rustc_no_writable]
pub fn as_mut_ptr(x: &mut [u8; 3]) -> *mut u8 {
    x as *mut [u8] as *mut u8
}
