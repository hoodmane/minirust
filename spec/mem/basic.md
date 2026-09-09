# MiniRust basic memory model

This is almost the simplest possible fully-feature implementation of the MiniRust memory model interface.
It does *not* model any kind of aliasing restriction, but otherwise should be enough to explain all the behavior and Undefined Behavior we see in Rust, in particular with respect to bounds-checks for memory accesses and pointer arithmetic.
This demonstrates well how the memory interface works, as well as the basics of "per-allocation provenance".
The full MiniRust memory model will likely be this basic model plus some [extra restrictions][Stacked Borrows] to ensure the program follows the aliasing rules; possibly with some extra tricks to [explain OOM-reducing optimizations](https://github.com/rust-lang/unsafe-code-guidelines/issues/328).

This memory model permits holding some "extra" data in each pointer and each allocation, so that code can be shared with more complicated models.

[Stacked Borrows]: https://github.com/rust-lang/unsafe-code-guidelines/blob/master/wip/stacked-borrows.md

## Data structures

The provenance tracked by this memory model is just an ID that identifies which allocation the pointer points to.
(We will pretend we can split the `impl ... for` block into multiple smaller blocks.)

```rust
pub struct AllocId(Int);

type Provenance<Extra> = (AllocId, Extra);
```

The data tracked by the memory is fairly simple: for each allocation, we track its data contents, its absolute integer address in memory, the alignment it was created with (the size is implicit in the length of the contents), and whether it is still alive (or has already been deallocated).

Allocations come in two flavors, one for each address space: ordinary byte memory, and externref table storage whose contents are whole externref values ("slots") rather than bytes.
Both spaces share the same allocation list (and hence the same provenance), but their addresses are entirely unrelated: a byte address and a table slot index never alias, no matter their numeric values.

```rust
enum AllocationData<ProvExtra = ()> {
    /// Ordinary byte memory.
    Bytes(List<AbstractByte<Provenance<ProvExtra>>>),
    /// Externref table storage: each cell is a slot storing a whole externref value.
    Table(List<ExternRefSlot>),
}

struct Allocation<ProvExtra = (), AllocExtra = ()> {
    /// The data stored in this allocation.
    /// For byte allocations, its length is measured in bytes; for externref
    /// table allocations, in slots.
    data: AllocationData<ProvExtra>,
    /// The address where this allocation starts.
    /// This is never 0, and `addr + data.len()` fits into a `usize`.
    addr: Address,
    /// The alignment that was requested for this allocation.
    /// `addr` will be a multiple of this.
    align: Align,
    /// The kind of this allocation.
    kind: AllocationKind,
    /// Whether this allocation is still live.
    live: bool,
    /// Additional information needed for the memory model
    extra: AllocExtra,
}
```

Memory then consists of a map tracking the allocation for each ID, stored as a list (since we assign IDs consecutively).

```rust
pub struct BasicMemory<T: Target, ProvExtra = (), AllocExtra = ()> {
    allocations: List<Allocation<ProvExtra, AllocExtra>>,

    // FIXME: specr should add this automatically
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Target, ProvExtra, AllocExtra> BasicMemory<T, ProvExtra, AllocExtra> {
    fn new() -> Self {
        Self { allocations: List::new(), _phantom: std::marker::PhantomData }
    }
}
```

## Operations

We start with some helper operations.

```rust
impl<ProvExtra> AllocationData<ProvExtra> {
    /// The number of units (bytes or table slots) stored in this allocation.
    fn len(self) -> Int {
        match self {
            AllocationData::Bytes(bytes) => bytes.len(),
            AllocationData::Table(slots) => slots.len(),
        }
    }

    /// Whether this is externref table storage.
    fn is_table(self) -> bool {
        match self {
            AllocationData::Bytes(_) => false,
            AllocationData::Table(_) => true,
        }
    }

    /// Extract the byte contents; the callers ensure (via `check_ptr`) that this
    /// is a byte allocation.
    fn expect_bytes(self, msg: &str) -> List<AbstractByte<Provenance<ProvExtra>>> {
        match self {
            AllocationData::Bytes(bytes) => bytes,
            AllocationData::Table(_) => panic!("expect_bytes: {msg}"),
        }
    }

    /// Extract the table contents; the callers ensure (via `check_ptr`) that this
    /// is an externref table allocation.
    fn expect_table(self, msg: &str) -> List<ExternRefSlot> {
        match self {
            AllocationData::Table(slots) => slots,
            AllocationData::Bytes(_) => panic!("expect_table: {msg}"),
        }
    }
}

impl<ProvExtra, AllocExtra> Allocation<ProvExtra, AllocExtra> {
    /// The size of this allocation, in the units of its address space
    /// (bytes for byte allocations, slots for externref table allocations).
    fn size(self) -> Size {
        Size::from_bytes(self.data.len()).unwrap()
    }

    fn overlaps(self, other_addr: Address, other_size: Size) -> bool {
        let end_addr = self.addr + self.size().bytes();
        let other_end_addr = other_addr + other_size.bytes();
        if end_addr <= other_addr || other_end_addr <= self.addr {
            // Our end is before their beginning, or vice versa -- we do not overlap.
            // However, to make sure that each allocation has a unique address, we still
            // report overlap if both allocations have the same address.
            // FIXME: This is not necessarily realistic, e.g. for zero-sized stack variables.
            // OTOH the function pointer logic currently relies on this.
            self.addr == other_addr
        } else {
            true
        }
    }
}
```

Then we implement creating and removing allocations.

```rust
impl<T: Target, ProvExtra, AllocExtra> BasicMemory<T, ProvExtra, AllocExtra> {
    fn allocate_inner(
        &mut self,
        kind: AllocationKind,
        data: AllocationData<ProvExtra>,
        align: Align,
        prov_extra: ProvExtra,
        alloc_extra: AllocExtra,
    ) -> NdResult<ThinPointer<Provenance<ProvExtra>>> {
        // The size, in the units of this allocation's address space.
        // The callers have already rejected invalid sizes.
        let size = Size::from_bytes(data.len()).unwrap();
        assert!(T::valid_size(size), "allocate_inner: callers ensure the size is valid");
        let is_table = data.is_table();
        // Pick a base address. We use daemonic non-deterministic choice,
        // meaning the program has to cope with every possible choice.
        // FIXME: This makes OOM (when there is no possible choice) into "no behavior",
        // which is not what we want.
        let distr = libspecr::IntDistribution {
            start: Int::ONE,
            end: Int::from(2).pow(T::PTR_SIZE.bits()),
            divisor: align.bytes(),
        };
        let addr = pick(distr, |addr: Address| {
            // Pick a strictly positive integer...
            if addr <= 0 { return false; }
            // ... that is suitably aligned...
            if !align.is_aligned(addr) { return false; }
            // ... such that addr+size is in-bounds of a `usize`...
            if !(addr+size.bytes()).in_bounds(Unsigned, T::PTR_SIZE) { return false; }
            // ... and it does not overlap with any existing live allocation in the same address space.
            // (Byte addresses and table slot indices are unrelated, so allocations
            // in different spaces may freely "overlap" numerically.)
            if self.allocations.any(|a| a.live && a.data.is_table() == is_table && a.overlaps(addr, size)) { return false; }
            // If all tests pass, we are good!
            true
        })?;

        // Compute allocation.
        let allocation = Allocation {
            addr,
            align,
            kind,
            live: true,
            data,
            extra: alloc_extra,
        };

        // Insert it into list, and remember where.
        let id = AllocId(self.allocations.len());
        self.allocations.push(allocation);

        // And we are done!
        ret(ThinPointer { addr, provenance: Some((id, prov_extra)) })
    }

    fn allocate(
        &mut self,
        kind: AllocationKind,
        size: Size,
        align: Align,
        prov_extra: ProvExtra,
        alloc_extra: AllocExtra,
    ) -> NdResult<ThinPointer<Provenance<ProvExtra>>> {
        // Reject too large allocations. Size must fit in `isize`.
        // (This must happen before we materialize the contents below.)
        if !T::valid_size(size) {
            throw_ub!("asking for a too large allocation");
        }
        self.allocate_inner(kind, AllocationData::Bytes(list![AbstractByte::Uninit; size.bytes()]), align, prov_extra, alloc_extra)
    }

    fn table_allocate(
        &mut self,
        kind: AllocationKind,
        count: Int,
        prov_extra: ProvExtra,
        alloc_extra: AllocExtra,
    ) -> NdResult<ThinPointer<Provenance<ProvExtra>>> {
        // The callers ensure that `count` is non-negative.
        // Reject too large allocations. The slot count must fit in `isize`.
        // (This must happen before we materialize the contents below.)
        if !T::valid_size(Size::from_bytes(count).unwrap()) {
            throw_ub!("asking for a too large allocation");
        }
        // Table slots have no alignment requirements, so the alignment is always 1.
        self.allocate_inner(kind, AllocationData::Table(list![ExternRefSlot::Uninit; count]), Align::ONE, prov_extra, alloc_extra)
    }

    fn deallocate(
        &mut self,
        ptr: ThinPointer<Provenance<ProvExtra>>,
        kind: AllocationKind,
        size: Size,
        align: Align,
        table: bool,
        handle_extra: impl FnOnce(&mut AllocExtra, ProvExtra) -> Result,
    ) -> Result {
        let Some((id, prov_extra)) = ptr.provenance else {
            throw_ub!("deallocating invalid pointer")
        };
        // This lookup will definitely work, since AllocId cannot be faked.
        let mut allocation = self.allocations[id.0];

        // Check a bunch of things.
        if !allocation.live {
            throw_ub!("double-free");
        }
        // Deallocation must happen in the right address space.
        if allocation.data.is_table() && !table {
            throw_ub!("byte memory access to an externref table allocation");
        }
        if !allocation.data.is_table() && table {
            throw_ub!("externref table access to a regular memory allocation");
        }
        if ptr.addr != allocation.addr {
            throw_ub!("deallocating with pointer not to the beginning of its allocation");
        }
        if kind != allocation.kind {
            throw_ub!("deallocating {:?} memory with {:?} deallocation operation", allocation.kind, kind);
        }
        if size != allocation.size() {
            throw_ub!("deallocating with incorrect size information");
        }
        if align != allocation.align {
            throw_ub!("deallocating with incorrect alignment information");
        }

        // Check "extra" things.
        handle_extra(&mut allocation.extra, prov_extra)?;

        // Mark it as dead.
        allocation.live = false;

        // That's it!
        self.allocations.set(id.0, allocation);

        ret(())
    }
}
```

The key operations of a memory model are of course handling loads and stores.
The helper function `check_ptr` we define for them is also used to implement the final part of the memory API, `dereferenceable`.

```rust
impl<T: Target, ProvExtra, AllocExtra> BasicMemory<T, ProvExtra, AllocExtra> {
    /// Check if the given pointer is dereferenceable for an access of the given
    /// length in the given address space (`table` indicates the externref table
    /// address space). For dereferenceable, return the allocation ID and
    /// offset; this can be missing for invalid pointers and accesses of size 0.
    fn check_ptr(&self, ptr: ThinPointer<Provenance<ProvExtra>>, len: Size, table: bool) -> Result<Option<(AllocId, ProvExtra, Size)>> {
        // For zero-sized accesses, there is nothing to check.
        // (Provenance monotonicity says that if we allow zero-sized accesses
        // for `None` provenance we have to allow it for all provenance.)
        if len.is_zero() {
            return ret(None);
        }
        // We do not even have to check for null, since no allocation will ever contain that address.
        // Now try to access the allocation information.
        let Some((id, prov_extra)) = ptr.provenance else {
            // An invalid pointer.
            throw_ub!("dereferencing pointer without provenance");
        };
        let allocation = self.allocations[id.0];
        if !allocation.live {
            throw_ub!("dereferencing pointer to dead allocation");
        }
        // The access must happen in the address space this allocation belongs to.
        if allocation.data.is_table() && !table {
            throw_ub!("byte memory access to an externref table allocation");
        }
        if !allocation.data.is_table() && table {
            throw_ub!("externref table access to a regular memory allocation");
        }

        // Compute relative offset, and ensure we are in-bounds.
        // We don't need a null ptr check, we just have an invariant that no allocation
        // contains the null address.
        let offset_in_alloc = ptr.addr - allocation.addr;
        if offset_in_alloc < 0 || offset_in_alloc + len.bytes() > allocation.size().bytes() {
            throw_ub!("dereferencing pointer outside the bounds of its allocation");
        }

        // All is good!
        ret(Some((id, prov_extra, Offset::from_bytes(offset_in_alloc).unwrap())))
    }

    fn store(
        &mut self,
        ptr: ThinPointer<Provenance<ProvExtra>>,
        bytes: List<AbstractByte<Provenance<ProvExtra>>>,
        align: Align,
        handle_extra: impl FnOnce(&mut AllocExtra, ProvExtra, Offset) -> Result,
    ) -> Result {
        if !align.is_aligned(ptr.addr) {
            throw_ub!("store to a misaligned pointer");
        }
        let size = Size::from_bytes(bytes.len()).unwrap();
        let Some((id, prov_extra, offset)) = self.check_ptr(ptr, size, /* table */ false)? else {
            return ret(());
        };
        let mut allocation = self.allocations[id.0];

        // Check and update "extra" state.
        handle_extra(&mut allocation.extra, prov_extra, offset)?;

        // Slice into the contents, and put the new bytes there.
        let mut data = allocation.data.expect_bytes("check_ptr ensures this is a byte allocation");
        data.write_subslice_at_index(offset.bytes(), bytes);
        allocation.data = AllocationData::Bytes(data);
        self.allocations.set(id.0, allocation);

        ret(())
    }

    fn load(
        &mut self,
        ptr: ThinPointer<Provenance<ProvExtra>>,
        len: Size,
        align: Align,
        handle_extra: impl FnOnce(&mut AllocExtra, ProvExtra, Offset) -> Result,
    ) -> Result<List<AbstractByte<Provenance<ProvExtra>>>> {
        if !align.is_aligned(ptr.addr) {
            throw_ub!("load from a misaligned pointer");
        }
        let Some((id, prov_extra, offset)) = self.check_ptr(ptr, len, /* table */ false)? else {
            return ret(list![]);
        };
        let mut allocation = self.allocations[id.0];

        // Check and update "extra" state.
        handle_extra(&mut allocation.extra, prov_extra, offset)?;
        self.allocations.set(id.0, allocation);

        // Slice into the contents, and copy them to a new list.
        ret(allocation.data.expect_bytes("check_ptr ensures this is a byte allocation").subslice_with_length(offset.bytes(), len.bytes()))
    }
}
```

The corresponding operations on the externref table address space mirror the byte operations, except that there are no alignment requirements and the contents are whole externref slots.

```rust
impl<T: Target, ProvExtra, AllocExtra> BasicMemory<T, ProvExtra, AllocExtra> {
    fn table_store(
        &mut self,
        ptr: ThinPointer<Provenance<ProvExtra>>,
        slots: List<ExternRefSlot>,
        handle_extra: impl FnOnce(&mut AllocExtra, ProvExtra, Offset) -> Result,
    ) -> Result {
        let count = Size::from_bytes(slots.len()).unwrap();
        let Some((id, prov_extra, offset)) = self.check_ptr(ptr, count, /* table */ true)? else {
            return ret(());
        };
        let mut allocation = self.allocations[id.0];

        // Check and update "extra" state.
        handle_extra(&mut allocation.extra, prov_extra, offset)?;

        // Slice into the contents, and put the new slots there.
        let mut data = allocation.data.expect_table("check_ptr ensures this is a table allocation");
        data.write_subslice_at_index(offset.bytes(), slots);
        allocation.data = AllocationData::Table(data);
        self.allocations.set(id.0, allocation);

        ret(())
    }

    fn table_load(
        &mut self,
        ptr: ThinPointer<Provenance<ProvExtra>>,
        count: Int,
        handle_extra: impl FnOnce(&mut AllocExtra, ProvExtra, Offset) -> Result,
    ) -> Result<List<ExternRefSlot>> {
        // The callers ensure that `count` is non-negative.
        let count = Size::from_bytes(count).unwrap();
        let Some((id, prov_extra, offset)) = self.check_ptr(ptr, count, /* table */ true)? else {
            return ret(list![]);
        };
        let mut allocation = self.allocations[id.0];

        // Check and update "extra" state.
        handle_extra(&mut allocation.extra, prov_extra, offset)?;
        self.allocations.set(id.0, allocation);

        // Slice into the contents, and copy them to a new list.
        ret(allocation.data.expect_table("check_ptr ensures this is a table allocation").subslice_with_length(offset.bytes(), count.bytes()))
    }
}
```

The memory leak check checks if there are any heap allocations left.
Stack allocations are fine; they get automatically cleaned up when a function returns and when the start function calls `exit`, its locals are still around.

```rust
impl<T: Target, ProvExtra, AllocExtra> BasicMemory<T, ProvExtra, AllocExtra> {
    fn leak_check(&self) -> Result {
        use AllocationKind::*;
        for allocation in self.allocations {
            if allocation.live {
                match allocation.kind {
                    // These should all be gone.
                    Heap => throw_memory_leak!(),
                    // These we can still have at the end.
                    Global | Function | Stack | VTable => {}
                }
            }
        }
        ret(())
    }
}
```

## Implementing the interface

The interface is now implemented fairly easily by forwarding to the operations declared above.

```rust
impl<T: Target> Memory for BasicMemory<T> {
    type Provenance = Provenance<()>;

    /// The target is given by the generic parameter.
    type T = T;

    /// The basic memory model does not need any per-frame data,
    /// so we set `FrameExtra` to the unit type.
    type FrameExtra = ();
    /// The basic memory model does not have any configuration parameters.
    type Params = ();

    fn new(_params: ()) -> Self {
        Self::new()
    }

    fn allocate(&mut self, kind: AllocationKind, size: Size, align: Align) -> NdResult<ThinPointer<Self::Provenance>> {
        self.allocate(kind, size, align, (), ())
    }

    fn deallocate(&mut self, ptr: ThinPointer<Self::Provenance>, kind: AllocationKind, size: Size, align: Align) -> Result {
        self.deallocate(ptr, kind, size, align, /* table */ false, |(), ()| ret(()))
    }

    fn store(&mut self, ptr: ThinPointer<Self::Provenance>, bytes: List<AbstractByte<Self::Provenance>>, align: Align) -> Result {
        self.store(ptr, bytes, align, |(), (), _offset| ret(()))
    }

    fn load(&mut self, ptr: ThinPointer<Self::Provenance>, len: Size, align: Align) -> Result<List<AbstractByte<Self::Provenance>>> {
        self.load(ptr, len, align, |(), (), _offset| ret(()))
    }

    fn dereferenceable(&self, ptr: ThinPointer<Self::Provenance>, len: Size) -> Result {
        self.check_ptr(ptr, len, /* table */ false)?;
        ret(())
    }

    fn table_allocate(&mut self, kind: AllocationKind, count: Int) -> NdResult<ThinPointer<Self::Provenance>> {
        self.table_allocate(kind, count, (), ())
    }

    fn table_deallocate(&mut self, ptr: ThinPointer<Self::Provenance>, kind: AllocationKind, count: Int) -> Result {
        // The callers ensure that `count` is non-negative.
        self.deallocate(ptr, kind, Size::from_bytes(count).unwrap(), Align::ONE, /* table */ true, |(), ()| ret(()))
    }

    fn table_store(&mut self, ptr: ThinPointer<Self::Provenance>, slots: List<ExternRefSlot>) -> Result {
        self.table_store(ptr, slots, |(), (), _offset| ret(()))
    }

    fn table_load(&mut self, ptr: ThinPointer<Self::Provenance>, count: Int) -> Result<List<ExternRefSlot>> {
        self.table_load(ptr, count, |(), (), _offset| ret(()))
    }

    fn table_dereferenceable(&self, ptr: ThinPointer<Self::Provenance>, count: Int) -> Result {
        // The callers ensure that `count` is non-negative.
        self.check_ptr(ptr, Size::from_bytes(count).unwrap(), /* table */ true)?;
        ret(())
    }

    fn is_table_provenance(&self, provenance: Self::Provenance) -> bool {
        let (id, ()) = provenance;
        self.allocations[id.0].data.is_table()
    }

    fn new_call() -> Self::FrameExtra {
        ()
    }

    fn leak_check(&self) -> Result {
        self.leak_check()
    }
}
```
