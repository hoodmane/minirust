#[repr(align(8))]
struct S {
    f: (),
}

fn main() {
    let mut x = &S { f: () };
    unsafe { (&raw mut x).cast::<usize>().write(0) };
    let _val = &x.f;
}
