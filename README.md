# micro-wibble garbage collector

A sesame-seed-sized garbage-collected heap, for tiny hardware.

It's simple, _not_ thread safe, and efficient for allocations up to about 512 bytes.

Here's an example of creating a 256-byte heap on the stack, using it to allocate two different objects, and then running the garbage collector to reap one of them:

```rust
use mwgc::{Heap, Memory};

let mut data: [u8; 256] = [0; 256];
let mut h = Heap::new(Memory::new(&mut data));
let o1 = h.allocate_object::<Sample>().unwrap();
let o2 = h.allocate_object::<Toaster>().unwrap();
h.gc(&[ o1 ]);
```


## usage



## how it works



Memory, Heap, allocate_object, gc
single threaded only
very dumb/simple

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
