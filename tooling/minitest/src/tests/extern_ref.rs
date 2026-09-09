use crate::*;

fn extern_ref_ptr_ty() -> Type {
    raw_ptr_ty(PointerMetaKind::None)
}

fn extern_fn(args: &[ExternTy], ret: ExternTy) -> ExternFunction {
    ExternFunction { args: args.iter().cloned().collect(), ret }
}

/// The unit return type for extern functions that return nothing.
fn extern_void() -> ExternTy {
    ExternTy::Other(<()>::get_type())
}

//
// Pass tests
//

/// f3-style call: `extern "C" { fn f() -> __externref_t; }`.
/// The return is lowered to a leading out-pointer; the fresh host ref is not null.
#[test]
fn host_new_is_not_null() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let locals = [extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        call(1, &[by_value(addr_of(local(0), extern_ref_ptr_ty()))], unit_place(), Some(1))
    );
    let b1 = block!(extern_ref_is_null(local(1), addr_of(local(0), extern_ref_ptr_ty()), 2));
    let b2 = block!(if_(load(local(1)), 3, 4));
    let b3 = block!(abort());
    let b4 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    dump_program(p);
    assert_stop::<BasicMem>(p);
}

/// The null externref written by `ExternRefWriteNull` is null.
#[test]
fn write_null_is_null() {
    let locals = [extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(extern_ref_is_null(local(1), addr_of(local(0), extern_ref_ptr_ty()), 2));
    let b2 = block!(if_(load(local(1)), 3, 4));
    let b3 = block!(exit());
    let b4 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// f1-style call: `extern "C" { fn f(x: __externref_t); }`.
/// The shim performs the table.get of the initialized slot; the host consumes the value.
#[test]
fn consume_arg() {
    let host_consume = extern_fn(&[ExternTy::ExternRef], extern_void());

    let locals = [extern_ref_ty()];
    let b0 =
        block!(storage_live(0), extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1));
    let b1 =
        block!(call(1, &[by_value(addr_of(local(0), extern_ref_ptr_ty()))], unit_place(), Some(2)));
    let b2 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2]);
    let p = program_with_extern_functions(&[f], &[host_consume]);
    assert_stop::<BasicMem>(p);
}

/// f2-style call: `extern "C" { fn f(x: *mut __externref_t); }`.
/// The table pointer is passed through unchanged; the slots need not even be initialized.
#[test]
fn pass_through_ptr() {
    let host_ptr_arg = extern_fn(&[ExternTy::ExternRefPtr], extern_void());

    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [arr_ty];
    let b0 = block!(
        storage_live(0),
        call(1, &[by_value(addr_of(local(0), extern_ref_ptr_ty()))], unit_place(), Some(1))
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_ptr_arg]);
    assert_stop::<BasicMem>(p);
}

/// f4-style call: `extern "C" { fn f() -> *mut __externref_t; }`.
/// The host returns a pointer to a fresh heap-region slot holding a fresh (non-null) ref,
/// which the program can inspect and deallocate.
#[test]
fn host_ptr_return() {
    let host_get_ptr = extern_fn(&[], ExternTy::ExternRefPtr);

    let locals = [extern_ref_ptr_ty(), <bool>::get_type()];
    let b0 = block!(storage_live(0), storage_live(1), call(1, &[], local(0), Some(1)));
    let b1 = block!(extern_ref_is_null(local(1), load(local(0)), 2));
    let b2 = block!(if_(load(local(1)), 3, 4));
    let b3 = block!(abort());
    let b4 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(1), 5));
    let b5 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5]);
    let p = program_with_extern_functions(&[f], &[host_get_ptr]);
    assert_stop::<BasicMem>(p);
}

/// `ExternRefCopy` copies a slot's content to another slot.
#[test]
fn copy_between_locals() {
    let locals = [extern_ref_ty(), extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(extern_ref_copy(
        addr_of(local(1), extern_ref_ptr_ty()),
        addr_of(local(0), extern_ref_ptr_ty()),
        2
    ));
    let b2 = block!(extern_ref_is_null(local(2), addr_of(local(1), extern_ref_ptr_ty()), 3));
    let b3 = block!(if_(load(local(2)), 4, 5));
    let b4 = block!(exit());
    let b5 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// Arrays of externref occupy consecutive table slots and support runtime indexing.
#[test]
fn array_indexing() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let arr_ty = array_ty(extern_ref_ty(), 3);
    let locals = [arr_ty, <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_write_null(
            addr_of(index(local(0), const_int::<usize>(0)), extern_ref_ptr_ty()),
            1
        )
    );
    // Let the host fill slot 1 via the out-pointer.
    let b1 = block!(call(
        1,
        &[by_value(addr_of(index(local(0), const_int::<usize>(1)), extern_ref_ptr_ty()))],
        unit_place(),
        Some(2)
    ));
    let b2 = block!(extern_ref_is_null(
        local(1),
        addr_of(index(local(0), const_int::<usize>(1)), extern_ref_ptr_ty()),
        3
    ));
    let b3 = block!(if_(load(local(1)), 4, 5));
    let b4 = block!(abort());
    let b5 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    assert_stop::<BasicMem>(p);
}

/// The table heap: allocate slots, use them as out-slots and copy targets, deallocate.
/// A clean exit also proves the allocation is not leaked.
#[test]
fn heap_table() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [extern_ref_ptr_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_allocate(const_int::<usize>(2), local(0), 1)
    );
    let b1 = block!(extern_ref_write_null(
        addr_of(index(deref(load(local(0)), arr_ty), const_int::<usize>(0)), extern_ref_ptr_ty()),
        2
    ));
    let b2 = block!(call(
        1,
        &[by_value(addr_of(
            index(deref(load(local(0)), arr_ty), const_int::<usize>(1)),
            extern_ref_ptr_ty()
        ))],
        unit_place(),
        Some(3)
    ));
    let b3 = block!(extern_ref_is_null(
        local(1),
        addr_of(index(deref(load(local(0)), arr_ty), const_int::<usize>(0)), extern_ref_ptr_ty()),
        4
    ));
    let b4 = block!(if_(load(local(1)), 5, 7));
    let b5 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(2), 6));
    let b6 = block!(exit());
    let b7 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5, b6, b7]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    assert_stop::<BasicMem>(p);
}

/// Extern signatures can mix ordinary types, raw externrefs, and table pointers.
#[test]
fn mixed_args() {
    // extern "C" { fn f(x: u32, y: __externref_t, z: *mut __externref_t) -> __externref_t; }
    let host_mixed = extern_fn(
        &[ExternTy::Other(<u32>::get_type()), ExternTy::ExternRef, ExternTy::ExternRefPtr],
        ExternTy::ExternRef,
    );

    let locals = [extern_ref_ty(), extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(call(
        1,
        &[
            // The leading out-pointer for the raw externref return.
            by_value(addr_of(local(1), extern_ref_ptr_ty())),
            by_value(const_int::<u32>(42)),
            by_value(addr_of(local(0), extern_ref_ptr_ty())),
            by_value(addr_of(local(0), extern_ref_ptr_ty())),
        ],
        unit_place(),
        Some(2)
    ));
    let b2 = block!(extern_ref_is_null(local(2), addr_of(local(1), extern_ref_ptr_ty()), 3));
    let b3 = block!(if_(load(local(2)), 4, 5));
    let b4 = block!(abort());
    let b5 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5]);
    let p = program_with_extern_functions(&[f], &[host_mixed]);
    assert_stop::<BasicMem>(p);
}

/// Safe references to externref work, and retagging them is a no-op:
/// the same program runs under both memory models.
fn ref_through_local_prog() -> Program {
    let ref_ty = ref_mut_ty_default_markers_for(extern_ref_ty());
    let locals = [extern_ref_ty(), ref_ty, <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(
        assign(local(1), addr_of(local(0), ref_ty)),
        // Retag the reference (a no-op for externref pointees).
        validate(local(1), false),
        extern_ref_is_null(local(2), load(local(1)), 2)
    );
    let b2 = block!(if_(load(local(2)), 3, 4));
    let b3 = block!(exit());
    let b4 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    program(&[f])
}

#[test]
fn ref_through_local() {
    let p = ref_through_local_prog();
    assert_stop::<BasicMem>(p);
    assert_stop::<TreeBorrowMem>(p);
}

/// A table pointer is an ordinary byte-representable value:
/// it can be stored in a struct in linear memory and loaded back.
#[test]
fn ptr_round_trip_through_bytes() {
    let ptr_in_tuple_ty = tuple_ty(&[(size(0), extern_ref_ptr_ty())], size(8), align(8));
    let locals = [extern_ref_ty(), ptr_in_tuple_ty, <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(
        // Store the table pointer into byte memory ...
        assign(field(local(1), 0), addr_of(local(0), extern_ref_ptr_ty())),
        // ... load it back, and use it to access the table.
        extern_ref_is_null(local(2), load(field(local(1), 0)), 2)
    );
    let b2 = block!(if_(load(local(2)), 3, 4));
    let b3 = block!(exit());
    let b4 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// StorageDead/StorageLive cycles work for externref locals.
#[test]
fn storage_cycles() {
    let locals = [extern_ref_ty()];
    let b0 =
        block!(storage_live(0), extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 1));
    let b1 = block!(
        storage_dead(0),
        storage_live(0),
        extern_ref_write_null(addr_of(local(0), extern_ref_ptr_ty()), 2)
    );
    let b2 = block!(storage_dead(0), exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

//
// UB tests
//

/// Passing an uninitialized slot at a raw externref position is UB
/// (the shim's table.get fails).
#[test]
fn consume_uninit_slot() {
    let host_consume = extern_fn(&[ExternTy::ExternRef], extern_void());

    let locals = [extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        call(1, &[by_value(addr_of(local(0), extern_ref_ptr_ty()))], unit_place(), Some(1))
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_consume]);
    assert_ub::<BasicMem>(p, "load of an uninitialized externref slot");
}

/// Testing an uninitialized slot for null-ness is UB.
#[test]
fn is_null_uninit_slot() {
    let locals = [extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_is_null(local(1), addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "load of an uninitialized externref slot");
}

/// Copying from an uninitialized slot is UB.
#[test]
fn copy_uninit_slot() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_copy(
            addr_of(local(1), extern_ref_ptr_ty()),
            addr_of(local(0), extern_ref_ptr_ty()),
            1
        )
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "load of an uninitialized externref slot");
}

/// Passing a byte-memory pointer at a raw externref position is UB (wrong address space).
#[test]
fn consume_byte_ptr() {
    let host_consume = extern_fn(&[ExternTy::ExternRef], extern_void());

    let locals = [<i32>::get_type()];
    let b0 = block!(
        storage_live(0),
        call(1, &[by_value(addr_of(local(0), raw_void_ptr_ty()))], unit_place(), Some(1))
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_consume]);
    assert_ub::<BasicMem>(p, "externref table access to a regular memory allocation");
}

/// Byte-typed accesses through a table pointer are UB (wrong address space).
#[test]
fn byte_access_to_table() {
    let locals = [extern_ref_ty(), <u8>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(1), load(deref(addr_of(local(0), extern_ref_ptr_ty()), <u8>::get_type()))),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "byte memory access to an externref table allocation");
}

/// In-bounds pointer arithmetic on a table pointer is UB:
/// table pointers do not support pointer arithmetic.
#[test]
fn ptr_offset_on_table_ptr() {
    let locals = [extern_ref_ty(), extern_ref_ptr_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(
            local(1),
            ptr_offset(
                addr_of(local(0), extern_ref_ptr_ty()),
                const_int::<usize>(1),
                InBounds::Yes
            )
        ),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "byte memory access to an externref table allocation");
}

/// A pointer without provenance can never access the externref table:
/// table pointers cannot be forged.
#[test]
fn no_provenance_table_access() {
    let locals = [<bool>::get_type()];
    let b0 = block!(storage_live(0), extern_ref_is_null(local(0), unit_ptr(), 1));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "dereferencing pointer without provenance");
}

/// Exposing the provenance of a table pointer is UB:
/// table pointers do not support integer casts.
#[test]
fn expose_table_ptr() {
    let locals = [extern_ref_ty(), <usize>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        expose_provenance(local(1), addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "exposing the provenance of an externref table pointer");
}

/// Using a saved table pointer after StorageDead is UB.
#[test]
fn use_after_storage_dead() {
    let locals = [extern_ref_ty(), extern_ref_ptr_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(1), addr_of(local(0), extern_ref_ptr_ty())),
        storage_dead(0),
        extern_ref_write_null(load(local(1)), 1)
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "dereferencing pointer to dead allocation");
}

/// Passing a dangling out-pointer to an externref-returning extern function is UB
/// (the shim's table.set fails).
#[test]
fn dangling_out_ptr() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let locals = [extern_ref_ptr_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<usize>(1), local(0), 1));
    let b1 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(1), 2));
    let b2 = block!(call(1, &[by_value(load(local(0)))], unit_place(), Some(3)));
    let b3 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    assert_ub::<BasicMem>(p, "dereferencing pointer to dead allocation");
}

/// Deallocating a table heap allocation twice is UB.
#[test]
fn double_table_free() {
    let locals = [extern_ref_ptr_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<usize>(1), local(0), 1));
    let b1 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(1), 2));
    let b2 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(1), 3));
    let b3 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "double-free");
}

/// Allocating a negative number of slots is UB.
#[test]
fn negative_alloc_count() {
    let locals = [extern_ref_ptr_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<isize>(-1), local(0), 1));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(
        p,
        "invalid slot count for `ExternRefAllocate` intrinsic: negative count",
    );
}

/// Out-of-bounds indexing into an externref array is UB.
#[test]
fn oob_index() {
    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [arr_ty];
    let b0 = block!(
        storage_live(0),
        extern_ref_write_null(
            addr_of(index(local(0), const_int::<usize>(2)), extern_ref_ptr_ty()),
            1
        )
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "access to out-of-bounds index");
}

/// Forgetting the leading out-pointer argument is an ABI violation.
#[test]
fn missing_out_arg() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let b0 = block!(call(1, &[], unit_place(), Some(1)));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    assert_ub::<BasicMem>(p, "call ABI violation: number of arguments does not agree");
}

/// Passing an integer at a raw externref position is an ABI violation
/// (the position takes a table pointer).
#[test]
fn int_at_externref_position() {
    let host_consume = extern_fn(&[ExternTy::ExternRef], extern_void());

    let b0 = block!(call(1, &[by_value(const_int::<usize>(0))], unit_place(), Some(1)));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_consume]);
    assert_ub::<BasicMem>(p, "call ABI violation: argument types are not compatible");
}

/// Calling an extern function with the Rust calling convention is an ABI violation.
#[test]
fn wrong_calling_convention() {
    let host_fn = extern_fn(&[], extern_void());

    let b0 = block!(Terminator::Call {
        callee: fn_ptr_internal(1),
        calling_convention: CallingConvention::Rust,
        arguments: list![],
        ret: unit_place(),
        next_block: Some(BbName(Name::from_internal(1))),
        unwind_block: None,
    });
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_fn]);
    assert_ub::<BasicMem>(p, "call ABI violation: calling conventions are not the same");
}

/// The MiniRust-level return type of an externref-returning extern function is unit;
/// anything else is an ABI violation.
#[test]
fn wrong_ret_place() {
    let host_new = extern_fn(&[], ExternTy::ExternRef);

    let locals = [<i32>::get_type()];
    let b0 = block!(storage_live(0), call(1, &[], local(0), Some(1)));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_new]);
    assert_ub::<BasicMem>(p, "call ABI violation: return types are not compatible");
}

/// Extern functions have no body the machine could run on a new thread.
#[test]
fn spawn_extern_fn() {
    let host_fn = extern_fn(&[], extern_void());

    let locals = [<usize>::get_type()];
    let b0 = block!(storage_live(0), spawn(fn_ptr_internal(1), unit_ptr(), local(0), 1));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program_with_extern_functions(&[f], &[host_fn]);
    assert_ub::<BasicMem>(p, "spawning an extern function");
}

/// Table heap allocations participate in the leak check.
#[test]
fn table_leak() {
    let locals = [extern_ref_ptr_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<usize>(1), local(0), 1));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_memory_leak::<BasicMem>(p);
}

//
// Ill-formed tests
//

/// Externref cannot be a tuple field (it has no byte layout).
#[test]
fn ill_formed_tuple_field() {
    let locals = [tuple_ty(&[(size(0), extern_ref_ty())], size(8), align(8))];
    let p = small_program(&locals, &[]);
    assert_ill_formed::<BasicMem>(p, "Type::Tuple: externref field type");
}

/// Externref cannot be the unsized tail of a tuple either (it counts as sized).
#[test]
fn ill_formed_tuple_tail() {
    let locals = [unsized_tuple_ty(&[], extern_ref_ty(), size(0), align(1), None)];
    let p = small_program(&locals, &[]);
    assert_ill_formed::<BasicMem>(p, "Type::Tuple: sized unsized field type");
}

/// Externref cannot be a union field.
#[test]
fn ill_formed_union_field() {
    let locals = [union_ty(&[(size(0), extern_ref_ty())], size(8), align(8))];
    let p = small_program(&locals, &[]);
    assert_ill_formed::<BasicMem>(p, "Type::Union: externref field type");
}

/// Externref cannot be an enum variant type.
#[test]
fn ill_formed_enum_variant() {
    let locals = [enum_ty::<u8>(
        &[(0, enum_variant(extern_ref_ty(), &[]))],
        discriminator_known(0),
        size(8),
        align(8),
    )];
    let p = small_program(&locals, &[]);
    assert_ill_formed::<BasicMem>(p, "Type::Enum: externref variant type");
}

/// Slices of externref are not supported (only arrays are).
#[test]
fn ill_formed_slice_elem() {
    let locals = [array_ty(slice_ty(extern_ref_ty()), 1)];
    let p = small_program(&locals, &[]);
    assert_ill_formed::<BasicMem>(p, "Type::Slice: externref element type");
}

/// Externref-typed places cannot be loaded: there are no externref values.
#[test]
fn ill_formed_load() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let stmts = [storage_live(0), storage_live(1), assign(local(1), load(local(0)))];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(p, "ValueExpr::Load: externref type");
}

/// Array aggregates of externref cannot be built: there are no externref values.
#[test]
fn ill_formed_array_aggregate() {
    let locals = [array_ty(extern_ref_ty(), 0)];
    let stmts = [storage_live(0), assign(local(0), array(&[], extern_ref_ty()))];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(p, "ValueExpr::Tuple: externref type");
}

/// Nothing can be transmuted into an externref.
#[test]
fn ill_formed_transmute_into() {
    let locals = [extern_ref_ty()];
    let stmts =
        [storage_live(0), assign(local(0), transmute(const_int::<usize>(0), extern_ref_ty()))];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(p, "Cast::Transmute: externref target type");
}

/// Externref types have no byte size to compute.
#[test]
fn ill_formed_compute_size() {
    let locals = [<usize>::get_type()];
    let stmts = [storage_live(0), assign(local(0), compute_size(extern_ref_ty(), unit()))];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(
        p,
        "UnOp::ComputeSize|ComputeAlign: externref types have no size or alignment",
    );
}

/// MiniRust functions cannot take externref arguments (there are no externref values);
/// extern functions take table pointers instead.
#[test]
fn ill_formed_fn_arg() {
    let locals = [extern_ref_ty()];
    let b0 = block!(return_());
    let f = function(Ret::No, 1, &locals, &[b0]);
    let p = program(&[f]);
    assert_ill_formed::<BasicMem>(p, "Function: externref argument or return type");
}

/// MiniRust functions cannot return externref either.
#[test]
fn ill_formed_fn_ret() {
    let locals = [extern_ref_ty()];
    let b0 = block!(return_());
    let f = function(Ret::Yes, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ill_formed::<BasicMem>(p, "Function: externref argument or return type");
}

/// Call arguments cannot have externref type (this covers in-place arguments,
/// which would otherwise sidestep the load ban).
#[test]
fn ill_formed_call_arg() {
    let callee_locals = [<usize>::get_type()];
    let callee_b0 = block!(return_());
    let callee = function(Ret::No, 1, &callee_locals, &[callee_b0]);

    let locals = [extern_ref_ty()];
    let b0 = block!(storage_live(0), call(1, &[in_place(local(0))], unit_place(), Some(1)));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f, callee]);
    assert_ill_formed::<BasicMem>(p, "Terminator::Call: externref argument type");
}

/// Call return places cannot have externref type.
#[test]
fn ill_formed_call_ret() {
    let callee_b0 = block!(return_());
    let callee = function(Ret::No, 0, &[], &[callee_b0]);

    let locals = [extern_ref_ty()];
    let b0 = block!(storage_live(0), call(1, &[], local(0), Some(1)));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f, callee]);
    assert_ill_formed::<BasicMem>(p, "Terminator::Call: externref return type");
}

/// Intrinsic return places cannot have externref type.
#[test]
fn ill_formed_intrinsic_ret() {
    let locals = [extern_ref_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<usize>(1), local(0), 1));
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ill_formed::<BasicMem>(p, "Terminator::Intrinsic: externref return type");
}

/// A raw externref in an extern signature must be declared as `ExternTy::ExternRef`,
/// not smuggled in as an ordinary type.
#[test]
fn ill_formed_extern_other_externref() {
    let host_fn = extern_fn(&[ExternTy::Other(extern_ref_ty())], extern_void());

    let b0 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0]);
    let p = program_with_extern_functions(&[f], &[host_fn]);
    assert_ill_formed::<BasicMem>(p, "ExternFunction: externref type in argument");
}

/// The minimal host can only produce externrefs; other return types must be unit.
#[test]
fn ill_formed_extern_ret() {
    let host_fn = extern_fn(&[], ExternTy::Other(<u32>::get_type()));

    let b0 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0]);
    let p = program_with_extern_functions(&[f], &[host_fn]);
    assert_ill_formed::<BasicMem>(p, "ExternFunction: unsupported return type");
}

/// Extern functions share the name space with regular functions; clashes are ill-formed.
#[test]
fn ill_formed_name_clash() {
    let host_fn = extern_fn(&[], extern_void());

    let b0 = block!(exit());
    let f = function(Ret::No, 0, &[], &[b0]);
    let mut p = program(&[f]);
    p.extern_functions = [(FnName(Name::from_internal(0)), host_fn)].into_iter().collect();
    assert_ill_formed::<BasicMem>(p, "Program: extern function name clashes with function");
}
