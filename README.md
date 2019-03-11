# micro-wibble garbage collector

A sesame-seed-sized heap with a tri-color, tracing, conservative, incremental, _non_-compacting garbage collector, for implementing a tiny language on tiny hardware.

It's simple, _not_ thread safe, and efficient for allocations up to about 512 bytes and heaps under about 1MB.

Here's an example of creating a 256-byte heap on the stack, using it to allocate two different objects, and then running the garbage collector to reap one of them:

```rust
use mwgc::{Heap, Memory};

let mut data: [u8; 256] = [0; 256];
let mut h = Heap::from_bytes(&mut data);
let o1 = h.allocate_object::<Sample>().unwrap();
let o2 = h.allocate_object::<Toaster>().unwrap();
h.gc(&[ o1 ]);
```


## Usage

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

You can "proactively" retire memory back into the heap if you want, although it's a little less efficient than letting the garbage collector run, because of some amortized work in the GC.

- `pub fn retire(&mut self, m: Memory<'heap>)`
- `pub fn retire_object<T>(&mut self, obj: &'heap mut T)`

There is also a function for the curious, which reports heap stats (total number of bytes, and total bytes free).

- `pub fn get_stats(&self) -> HeapStats`


## How it works

The heap is organized into 16-byte (configurable via compile-time constant) blocks. Each block has a 2-bit "color" in a bitmap carved out of the end of the heap region, at a cost of about 2% overhead. One "color" is used to mark continuations of contiguous spans, so each allocation can be identified by a color followed by one or more "continue" markers. The other 3 colors correspond to the white, gray, and black colors of tri-color marking, although I've chosen to call them blue, green, and "check".


There is a fair amount of `unsafe` code in this library. I've tried to isolate most of it into a few helper functions, but the concept of a garbage collected heap allows and requires several features that rust's borrow checker is explicitly designed to prevent. :)





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
