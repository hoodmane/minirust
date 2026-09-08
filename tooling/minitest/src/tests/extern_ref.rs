use crate::*;

fn extern_ref_ptr_ty() -> Type {
    raw_ptr_ty(PointerMetaKind::None)
}

//
// Pass tests
//

/// A fresh externref from the host is not null.
#[test]
fn new_is_not_null() {
    let locals = [extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(storage_live(0), storage_live(1), extern_ref_new(local(0), 1));
    let b1 = block!(extern_ref_is_null(local(1), load(local(0)), 2));
    let b2 = block!(if_(load(local(1)), 3, 4));
    let b3 = block!(abort());
    let b4 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program(&[f]);
    dump_program(p);
    assert_stop::<BasicMem>(p);
}

/// The null externref constant is null.
#[test]
fn null_is_null() {
    let locals = [extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(0), const_extern_ref_null()),
        extern_ref_is_null(local(1), load(local(0)), 1)
    );
    let b1 = block!(if_(load(local(1)), 2, 3));
    let b2 = block!(exit());
    let b3 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// Externref locals can be reassigned and copied like any other local.
#[test]
fn reassign_local() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(0), const_extern_ref_null()),
        extern_ref_new(local(0), 1)
    );
    let b1 =
        block!(assign(local(1), load(local(0))), assign(local(0), const_extern_ref_null()), exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// Taking a reference to an externref local and reading/writing through it works,
/// under both memory models (Tree Borrows retagging is a no-op for table pointers).
fn ref_through_local_prog() -> Program {
    let ref_ty = ref_mut_ty_default_markers_for(extern_ref_ty());
    let locals = [extern_ref_ty(), ref_ty, extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        storage_live(3),
        extern_ref_new(local(0), 1)
    );
    let b1 = block!(
        assign(local(1), addr_of(local(0), ref_ty)),
        // Write the null externref through the reference ...
        assign(deref(load(local(1)), extern_ref_ty()), const_extern_ref_null()),
        // ... and read it back through the reference.
        assign(local(2), load(deref(load(local(1)), extern_ref_ty()))),
        extern_ref_is_null(local(3), load(local(2)), 2)
    );
    let b2 = block!(if_(load(local(3)), 3, 4));
    let b3 = block!(exit());
    let b4 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    program(&[f])
}

#[test]
fn ref_through_local() {
    let p = ref_through_local_prog();
    dump_program(p);
    assert_stop::<BasicMem>(p);
    assert_stop::<TreeBorrowMem>(p);
}

/// Arrays of externref occupy consecutive table slots and support runtime indexing,
/// including whole-array copies.
#[test]
fn array_indexing() {
    let arr_ty = array_ty(extern_ref_ty(), 3);
    let locals = [arr_ty, arr_ty, <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        assign(index(local(0), const_int::<usize>(0)), const_extern_ref_null()),
        assign(index(local(0), const_int::<usize>(2)), const_extern_ref_null()),
        extern_ref_new(index(local(0), const_int::<usize>(1)), 1)
    );
    let b1 = block!(
        // Copy the whole array (a 3-slot table load and store).
        assign(local(1), load(local(0))),
        extern_ref_is_null(local(2), load(index(local(1), const_int::<usize>(1))), 2)
    );
    let b2 = block!(if_(load(local(2)), 3, 4));
    let b3 = block!(abort());
    let b4 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// The table heap: allocate slots, write/read them through the pointer, deallocate.
/// A clean exit also proves the allocation is not leaked.
#[test]
fn heap_table() {
    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [extern_ref_ptr_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        extern_ref_allocate(const_int::<usize>(2), local(0), 1)
    );
    let b1 = block!(
        assign(
            index(deref(load(local(0)), arr_ty), const_int::<usize>(0)),
            const_extern_ref_null()
        ),
        extern_ref_new(index(deref(load(local(0)), arr_ty), const_int::<usize>(1)), 2)
    );
    let b2 = block!(extern_ref_is_null(
        local(1),
        load(index(deref(load(local(0)), arr_ty), const_int::<usize>(0))),
        3
    ));
    let b3 = block!(if_(load(local(1)), 4, 6));
    let b4 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(2), 5));
    let b5 = block!(exit());
    let b6 = block!(abort());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5, b6]);
    let p = program(&[f]);
    dump_program(p);
    assert_stop::<BasicMem>(p);
}

/// A pointer to an externref is an ordinary byte-representable value:
/// it can be stored in a struct in linear memory and loaded back.
#[test]
fn ptr_round_trip_through_bytes() {
    let ptr_in_tuple_ty = tuple_ty(&[(size(0), extern_ref_ptr_ty())], size(8), align(8));
    let locals = [extern_ref_ty(), ptr_in_tuple_ty, extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        storage_live(2),
        storage_live(3),
        extern_ref_new(local(0), 1)
    );
    let b1 = block!(
        // Store the table pointer into byte memory ...
        assign(field(local(1), 0), addr_of(local(0), extern_ref_ptr_ty())),
        // ... load it back, and dereference it into the table.
        assign(local(2), load(deref(load(field(local(1), 0)), extern_ref_ty()))),
        extern_ref_is_null(local(3), load(local(2)), 2)
    );
    let b2 = block!(if_(load(local(3)), 3, 4));
    let b3 = block!(abort());
    let b4 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

/// Externref can be passed as a by-value argument and returned from a function.
#[test]
fn arg_and_return() {
    // fn id(x: externref) -> externref { x }
    let id_locals = [extern_ref_ty(), extern_ref_ty()];
    let id_b0 = block!(assign(local(0), load(local(1))), return_());
    let id_fn = function(Ret::Yes, 1, &id_locals, &[id_b0]);

    let locals = [extern_ref_ty(), extern_ref_ty(), <bool>::get_type()];
    let b0 = block!(storage_live(0), storage_live(1), storage_live(2), extern_ref_new(local(0), 1));
    let b1 = block!(call(1, &[by_value(load(local(0)))], local(1), Some(2)));
    let b2 = block!(extern_ref_is_null(local(2), load(local(1)), 3));
    let b3 = block!(if_(load(local(2)), 4, 5));
    let b4 = block!(abort());
    let b5 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2, b3, b4, b5]);
    let p = program(&[f, id_fn]);
    assert_stop::<BasicMem>(p);
}

/// Externref can also be passed as an in-place argument
/// (which de-initializes the caller's table slot).
#[test]
fn arg_in_place() {
    // fn id(x: externref) -> externref { x }
    let id_locals = [extern_ref_ty(), extern_ref_ty()];
    let id_b0 = block!(assign(local(0), load(local(1))), return_());
    let id_fn = function(Ret::Yes, 1, &id_locals, &[id_b0]);

    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(storage_live(0), storage_live(1), extern_ref_new(local(0), 1));
    let b1 = block!(call(1, &[in_place(local(0))], local(1), Some(2)));
    let b2 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2]);
    let p = program(&[f, id_fn]);
    assert_stop::<BasicMem>(p);
}

/// StorageDead/StorageLive cycles work for externref locals.
#[test]
fn storage_cycles() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(storage_live(0), storage_live(1), extern_ref_new(local(0), 1));
    let b1 = block!(
        storage_dead(0),
        storage_live(0),
        assign(local(0), const_extern_ref_null()),
        assign(local(1), load(local(0))),
        storage_dead(0),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_stop::<BasicMem>(p);
}

//
// UB tests
//

/// Reading an externref local before it was initialized is UB.
#[test]
fn read_uninit_local() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(storage_live(0), storage_live(1), assign(local(1), load(local(0))), exit());
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "load of an uninitialized externref slot");
}

/// Loading a whole array where one slot is still uninitialized is UB.
#[test]
fn read_partially_uninit_array() {
    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [arr_ty, arr_ty];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(index(local(0), const_int::<usize>(0)), const_extern_ref_null()),
        // slot 1 is never written
        assign(local(1), load(local(0))),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "load of an uninitialized externref slot");
}

/// Accessing an externref local that is not live is UB.
#[test]
fn use_dead_local() {
    let locals = [extern_ref_ty()];
    let b0 = block!(assign(local(0), const_extern_ref_null()), exit());
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "access to a dead local");
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
        assign(deref(load(local(1)), extern_ref_ty()), const_extern_ref_null()),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "dereferencing pointer to dead allocation");
}

/// Using a table heap pointer after deallocation is UB.
#[test]
fn use_after_table_dealloc() {
    let locals = [extern_ref_ptr_ty()];
    let b0 = block!(storage_live(0), extern_ref_allocate(const_int::<usize>(1), local(0), 1));
    let b1 = block!(extern_ref_deallocate(load(local(0)), const_int::<usize>(1), 2));
    let b2 =
        block!(assign(deref(load(local(0)), extern_ref_ty()), const_extern_ref_null()), exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1, b2]);
    let p = program(&[f]);
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

/// A pointer without provenance can never access the externref table:
/// table pointers cannot be forged from integers.
#[test]
fn deref_no_provenance() {
    let locals = [extern_ref_ty()];
    let b0 =
        block!(storage_live(0), assign(local(0), load(deref(unit_ptr(), extern_ref_ty()))), exit());
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "dereferencing pointer without provenance");
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

/// Externref-typed accesses through a byte memory pointer are UB (wrong address space).
#[test]
fn table_access_to_bytes() {
    let locals = [<i32>::get_type(), extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(1), load(deref(addr_of(local(0), raw_void_ptr_ty()), extern_ref_ty()))),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "externref table access to a regular memory allocation");
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

/// Out-of-bounds indexing into an externref array is UB.
#[test]
fn oob_index() {
    let arr_ty = array_ty(extern_ref_ty(), 2);
    let locals = [arr_ty];
    let b0 = block!(
        storage_live(0),
        assign(index(local(0), const_int::<usize>(2)), const_extern_ref_null()),
        exit()
    );
    let f = function(Ret::No, 0, &locals, &[b0]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "access to out-of-bounds index");
}

/// Atomic loads at externref type are UB: externref has no byte representation.
#[test]
fn atomic_load_externref() {
    let locals = [extern_ref_ty(), extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        storage_live(1),
        assign(local(0), const_extern_ref_null()),
        atomic_load(local(1), addr_of(local(0), extern_ref_ptr_ty()), 1)
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "invalid return type for `AtomicLoad` intrinsic: unsized type");
}

/// Atomic stores of externref values are UB: externref has no byte representation.
#[test]
fn atomic_store_externref() {
    let locals = [extern_ref_ty()];
    let b0 = block!(
        storage_live(0),
        assign(local(0), const_extern_ref_null()),
        atomic_store(addr_of(local(0), extern_ref_ptr_ty()), load(local(0)), 1)
    );
    let b1 = block!(exit());
    let f = function(Ret::No, 0, &locals, &[b0, b1]);
    let p = program(&[f]);
    assert_ub::<BasicMem>(p, "invalid second argument to `AtomicStore` intrinsic: unsized type");
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

/// Externref cannot be transmuted away: it has no bytes to reinterpret.
#[test]
fn ill_formed_transmute_from() {
    let locals = [extern_ref_ty(), <usize>::get_type()];
    let stmts = [
        storage_live(0),
        storage_live(1),
        assign(local(0), const_extern_ref_null()),
        assign(local(1), transmute(load(local(0)), <usize>::get_type())),
    ];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(p, "Cast::Transmute: externref source type");
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

/// The null externref constant only has type `externref`.
#[test]
fn ill_formed_null_at_wrong_type() {
    let locals = [<i32>::get_type()];
    let stmts = [
        storage_live(0),
        assign(local(0), ValueExpr::Constant(Constant::ExternRefNull, <i32>::get_type())),
    ];
    let p = small_program(&locals, &stmts);
    assert_ill_formed::<BasicMem>(p, "Constant: value does not match type");
}
