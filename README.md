# micro-wibble garbage collector

A sesame-seed-sized garbage-collected heap, for tiny hardware.

It's simple, _not_ thread safe, and efficient for allocations up to about 512 bytes.

Here's an example of creating a 256-byte heap on the stack, using it to allocate two different objects, and then running the garbage collector to reap one of them:

```rust
use mwgc::{Heap, Memory};

let mut data: [u8; 256] = [0; 256];
let mut h = Heap::from_bytes(&mut data);
let o1 = h.allocate_object::<Sample>().unwrap();
let o2 = h.allocate_object::<Toaster>().unwrap();
h.gc(&[ o1 ]);
```


## usage

The `Heap` takes ownership of a block of `Memory` (which is a wrapper for a mutable byte slice `&mut [u8]`), and hands out chunks of it on demand.

- `pub fn new(m: Memory<'heap>) -> Heap<'heap>`
- `pub fn allocate(&mut self, amount: usize) -> Option<Memory<'heap>>`

`allocate` may return `None` if there isn't enough room left in the heap.

For convenience, you can create a `Heap` directly out of a mutable byte slice, and ask for objects or arrays of a known size.

- `pub fn from_bytes(bytes: &'heap mut [u8]) -> Heap<'heap>`
- `pub fn allocate_object<T: Default>(&mut self) -> Option<&'heap mut T>`
- `pub fn allocate_array<T: Default>(&mut self, count: usize) -> Option<&'heap mut [T]>`

To free unused memory, run the garbage collector:

- `pub fn gc<T>(&mut self, roots: &[&T])`

Any reference that can't be traced from the set of roots (passed in as an array slice of arbitrary references) will be marked as free, and become available to future `allocate` calls.

If you're worried about latency, you can also run the garbage collector as incremental steps:

- `pub fn mark<T>(&mut self, roots: &[&T])`
- `pub fn sweep(&mut self)`

The mark phase can also be broken down into steps. The first step only marks objects directly accessible from the roots. Each later round goes one layer deeper into the tree of references. `mark_round` must be called until it returns `true`, which indicates that it's finished traversing every accessible object.

- `pub fn mark_start<T>(&mut self, roots: &[&T])`
- `pub fn mark_round(&mut self) -> bool`

**Important**: You can allocate new objects between each step of garbage collection, but all "live" references must be reachable from the roots each time you call the next step.




free manually

    pub fn get_stats(&self) -> HeapStats


## how it works




follow references conservatively

```rust

```

## stages

- quiescent (GC isn't running)
    - current state: blue or green
    - new allocs: mark with current state

- mark
    - current state: blue or green; "next color" is the other color
    - new allocs: mark as "next color" (grey?)
    - there are no "hidden references": all live object references must be on the stack (and therefore accessed via roots)
    - algorithm:
        1. track "current range" (initially empty). as items are marked, if they are outside the range, expand it.
        2. mark all roots as gray.
        3. walk the current range. for each gray:
            1. mark the children gray.
            2. mark the object "next color".
            3. if the cursor is the same as the range start, move the range start to follow the cursor.
        4. if the range is not empty, repeat 3.

- sweep
    - current state: blue or green, the opposite color ("next color") from the mark stage
    - new allocs: mark with current state
    - algorithm:
        1. walk the entire range. add any "old color" block to the free list.

## problem

- when marking, we only have a pointer, but we need to find the extent of the allocation.
- maybe mark all free space with a run of blue/green/check (anything but "cont") so that we can find the end of an allocation by scanning forward in the colormap?
