# micro-wibble garbage collector

A sesame-seed-sized heap with a tri-color, tracing, conservative, incremental, _non_-compacting garbage collector, for implementing a tiny language on tiny hardware.

It's simple, _not_ thread safe, and efficient for allocations up to about 512 bytes and heaps under about 1MB.

Here's an example of creating a 256-byte heap on the stack, using it to allocate two different objects, and then running the garbage collector to reap one of them:

```rust
use mwgc::Heap;

let mut data: [u8; 256] = [0; 256];
let mut h = Heap::from_bytes(&mut data);
let o1 = h.allocate_object::<Bread>().unwrap();
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

**Important**: You can allocate new objects between each step of garbage collection, but all "live" references must be reachable from the roots each time you call the next step.

The mark phase can also be broken down into steps. The first step only marks objects directly accessible from the roots. Each later round goes one layer deeper into the tree of references. `mark_round` must be called until it returns `true`, which indicates that it's finished traversing every accessible object.

- `pub fn mark_start<T>(&mut self, roots: &[&T])`
- `pub fn mark_round(&mut self) -> bool`
- `pub fn mark_check<T>(&mut self, obj: &T)`

**Important**: If you modify any objects after `mark_start`, before `mark_round` returns `true`, you must notify the garbage collector to (re)check the object you modified by calling `mark_check`. This is the price of such fine granularity.

You can "proactively" retire memory back into the heap if you want, although it's a little less efficient than letting the garbage collector run, because of some amortized work in the GC.

- `pub fn retire(&mut self, m: Memory<'heap>)`
- `pub fn retire_object<T>(&mut self, obj: &'heap mut T)`

There is also a function for the curious, which reports heap stats (total number of bytes, and total bytes free).

- `pub fn get_stats(&self) -> HeapStats`

There is a fair amount of `unsafe` code in this library. I've tried to isolate most of it into a few helper functions, but the concept of a garbage collected heap allows and requires several features that rust's borrow checker is explicitly designed to prevent. :)


## How it works

The heap is organized into allocatable blocks, with a small region reserved as a bitmap. The block size is configured by a compile-time constant, using a default of 16 bytes. Each block has a 2-bit "color" in the bitmap, at a cost of about 2% overhead. One "color" is used to mark continuations of contiguous spans, so each allocation can be identified by a color followed by one or more "continue" markers. The other 3 colors correspond to the white, gray, and black colors of tri-color marking, although I've chosen to call them blue, green, and "check".

Separately, a sorted free list is maintained by storing a "next" link and size in each free block. This consumes 8 bytes on a 32-bit system, limiting the minimum block size.

At startup, live memory is marked as blue. During GC, the mark phase will mark all live spans as green, and the sweep phase will add the remaining blue spans into the free list. For the next GC, the colors will be reversed, with live objects being marked as blue again.

Marking a span causes its color to change to "check". The heap tracks the start and end range of spans that have been marked, so each `mark_round` call traverses that range, looking for words that are aligned and appear to point within the heap. Any such spans are colored "check" and added to the new range. In this way, each round is approximately one level deeper into the object graph, although it may jump ahead if the links are in increasing order.


## License

Apache 2 (open-source) license, included in `LICENSE.txt`.


## Authors

- Robey Pointer <robeypointer@gmail.com> / @robey@mastodon.technology / github @robey
