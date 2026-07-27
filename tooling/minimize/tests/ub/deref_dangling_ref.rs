#[repr(C)]
struct S {
    a: (),
    b: i8,
}

fn main() {
    let mut x = &S { a: (), b: 0 };
    unsafe { (&raw mut x).cast::<usize>().write(16) };
    let _val = x.a;
}
