

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
