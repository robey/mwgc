use core::mem;
use mwgc::{Heap, Memory};

#[repr(align(8))]
struct Blob {
    data: [u8; 256]
}

static mut DATA: Blob = Blob { data: [0; 256] };

// used to test the GC
#[derive(Default)]
struct Sample<'a> {
    p: Option<&'a Sample<'a>>,
    number: usize,
    next: Option<&'a Sample<'a>>,
    prev: Option<&'a Sample<'a>>,
}

impl<'a> Sample<'a> {
    pub fn ptr(&self) -> *const u8 {
        self as *const Sample as *const u8
    }

    pub fn as_mut(&self) -> &'a mut Sample<'a> {
        unsafe { &mut *(self as *const Sample as *mut Sample) }
    }
}


#[test]
fn new_heap() {
    let mut data: [u8; 256] = [0; 256];
    let start = &data[0] as *const u8;
    let h = Heap::new(Memory::new(&mut data));
    assert_eq!(h.get_stats().start, start);
    assert_eq!(h.get_stats().end, unsafe { h.get_stats().start.offset(240) });

    let mut buffer: [u8; 256] = [0; 256];
    assert_eq!(h.dump_into(&mut buffer), "FREE[240]");
}

#[test]
fn allocate() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let alloc = h.allocate(32);
    assert!(alloc.is_some());
    if let Some(m) = alloc {
        assert_eq!(m.len(), 32);

        let mut buffer: [u8; 256] = [0; 256];
        assert_eq!(h.dump_into(&mut buffer), "Blue[32], FREE[208]");
    }
}

#[test]
fn allocate_array() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let array = h.allocate_array::<u32>(10);
    assert!(array.is_some());
    if let Some(a) = array {
        assert_eq!(a.len(), 10);

        // multiple of 16:
        let mut buffer: [u8; 256] = [0; 256];
        assert_eq!(h.dump_into(&mut buffer), "Blue[48], FREE[192]");
    }
}

#[test]
fn retire() {
    let mut data: [u8; 256] = [0; 256];
    let mut buffer: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let m1 = h.allocate(32).unwrap();
    let m2 = h.allocate(32).unwrap();
    h.retire(m1);
    assert_eq!(h.dump_into(&mut buffer), "FREE[32], Blue[32], FREE[176]");
    h.retire(m2);
    assert_eq!(h.dump_into(&mut buffer), "FREE[240]");

    let m3 = h.allocate_object::<Sample>().unwrap();
    assert_eq!(
        h.dump_into(&mut buffer),
        format!("Blue[{}], FREE[{}]", mem::size_of::<Sample>(), 240 - mem::size_of::<Sample>())
    );
    h.retire_object(m3);
    assert_eq!(h.dump_into(&mut buffer), "FREE[240]");
}

#[test]
fn mark_simple() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let o1 = h.allocate_object::<Sample>().unwrap();
    let o2 = h.allocate_object::<Sample>().unwrap();
    let _o3 = h.allocate_object::<Sample>().unwrap();
    let o4 = h.allocate_object::<Sample>().unwrap();
    let o5 = h.allocate_object::<Sample>().unwrap();

    let mut buffer: [u8; 256] = [0; 256];
    assert_eq!(h.dump_spans_into(&mut buffer), "Blue, Blue, Blue, Blue, Blue, FREE");

    // leave o3 stranded. make o1 point to o2, which points to o4, o5, and back to o1.
    o4.p = Some(unsafe { &*(455 as *const Sample) });
    o5.number = 23;
    o2.p = Some(o4);
    o2.next = Some(o5);
    o2.prev = Some(unsafe { &*(o1 as *const Sample) });  // trick rust into making a circ ref
    o1.p = Some(o2);

    h.mark_start(&[ o1 ]);
    assert_eq!(h.get_mark_range(), (o1.ptr(), o1.ptr()));
    assert_eq!(h.dump_spans_into(&mut buffer), "Check, Blue, Blue, Blue, Blue, FREE");

    assert!(!h.mark_round());
    assert_eq!(h.get_mark_range(), (o2.ptr(), o2.ptr()));
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Check, Blue, Blue, Blue, FREE");

    assert!(!h.mark_round());
    assert_eq!(h.get_mark_range(), (o4.ptr(), o5.ptr()));
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Blue, Check, Check, FREE");

    assert!(h.mark_round());
    assert_eq!(h.get_mark_range(), (core::ptr::null(), core::ptr::null()));
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Blue, Green, Green, FREE");
}

#[test]
fn sweep_simple() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let o1 = h.allocate_object::<Sample>().unwrap();
    let _o2 = h.allocate_object::<Sample>().unwrap();
    let o3 = h.allocate_object::<Sample>().unwrap();
    let _o4 = h.allocate_object::<Sample>().unwrap();
    let _o5 = h.allocate_object::<Sample>().unwrap();

    let mut buffer: [u8; 256] = [0; 256];
    assert_eq!(h.dump_spans_into(&mut buffer), "Blue, Blue, Blue, Blue, Blue, FREE");

    o1.p = Some(o3);
    h.mark(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Blue, Green, Blue, Blue, FREE");
    h.sweep();
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, FREE, Green, FREE");

    o1.p = None;
    h.mark(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Blue, FREE, Green, FREE");
    h.sweep();
    assert_eq!(h.dump_spans_into(&mut buffer), "Blue, FREE");
}

#[test]
fn alloc_during_collection() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let mut buffer: [u8; 256] = [0; 256];

    // start with o1 -> o2 -> o3.
    let o1 = h.allocate_object::<Sample>().unwrap();
    let o2 = h.allocate_object::<Sample>().unwrap();
    let o3 = h.allocate_object::<Sample>().unwrap();
    o2.p = Some(o3);
    o1.p = Some(o2);

    h.mark_start(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Check, Blue, Blue, FREE");

    assert_eq!(h.mark_round(), false);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Check, Blue, FREE");

    // o1 is saved, o2 will be checked on the next round. so, let's
    // allocate an o4, and move the links to be: o2 -> o4 -> o3.
    let o4 = h.allocate_object::<Sample>().unwrap();
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Check, Blue, Check, FREE");
    o4.p = Some(o3);
    let o2 = o1.p.take().unwrap().as_mut();
    o2.p = Some(o4);

    assert_eq!(h.mark_round(), false);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Check, Green, FREE");

    assert_eq!(h.mark_round(), true);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Green, Green, FREE");
}

#[test]
fn inner_pointer() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let mut buffer: [u8; 256] = [0; 256];

    // o1 points to inside o3.
    let o1 = h.allocate_object::<Sample>().unwrap();
    let _o2 = h.allocate_object::<Sample>().unwrap();
    let o3 = h.allocate_object::<Sample>().unwrap();
    let inside_o3 = unsafe { &*(((o3 as *const Sample as usize) + mem::size_of::<usize>() * 2) as *const Sample) };
    o1.p = Some(inside_o3);

    h.gc(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, FREE, Green, FREE");
}

#[test]
fn ref_moves_backward() {
    let mut data: [u8; 256] = [0; 256];
    let mut h = Heap::new(Memory::new(&mut data));
    let mut buffer: [u8; 256] = [0; 256];

    // start with o1 -> o2 -> o3.
    let o1 = h.allocate_object::<Sample>().unwrap();
    let o2 = h.allocate_object::<Sample>().unwrap();
    let o3 = h.allocate_object::<Sample>().unwrap();
    o2.p = Some(o3);
    o1.p = Some(o2);

    h.mark_start(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Check, Blue, Blue, FREE");

    assert_eq!(h.mark_round(), false);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Check, Blue, FREE");

    // suddenly it's o1 -> o3 -> o2.
    let o2 = o1.p.take().unwrap().as_mut();
    let o3 = o2.p.take().unwrap().as_mut();
    o3.p = Some(o2);
    o1.p = Some(o3);
    h.mark_check(o1);
    assert_eq!(h.dump_spans_into(&mut buffer), "Check, Check, Blue, FREE");

    assert_eq!(h.mark_round(), false);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Check, FREE");

    assert_eq!(h.mark_round(), true);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, Green, Green, FREE");
}

#[test]
fn api() {
    let mut h = Heap::new(Memory::new(unsafe { &mut DATA.data }));
    let o1 = h.allocate_object::<Sample>().unwrap();
    let _o2 = h.allocate_object::<Sample>().unwrap();
    let o3 = h.allocate_object::<Sample>().unwrap();
    let o4 = h.allocate_object::<Sample>().unwrap();
    let _o5 = h.allocate_object::<Sample>().unwrap();
    o3.number = (o4.ptr() as usize) + 1;
    o1.p = Some(o3);
    let stats = h.get_stats();
    assert_eq!(stats.total_bytes, 240);
    assert_eq!(stats.free_bytes, 240 - 5 * mem::size_of::<Sample>());

    assert!(h.size_of(o1) >= mem::size_of::<Sample>());

    let mut buffer: [u8; 256] = [0; 256];

    h.gc(&[ o1 ]);
    assert_eq!(h.dump_spans_into(&mut buffer), "Green, FREE, Green, FREE");
    let stats2 = h.get_stats();
    assert_eq!(stats2.total_bytes, 240);
    assert_eq!(stats2.free_bytes, 240 - 2 * mem::size_of::<Sample>());
}

#[test]
fn safe_pointers() {
    let h = Heap::new(Memory::new(unsafe { &mut DATA.data }));
    let (start, end) = h.get_live_range();

    assert_eq!(h.is_ptr_inside(start as *const usize), true);
    assert_eq!(h.is_ptr_inside((start - 1) as *const usize), false);
    assert_eq!(h.is_ptr_inside((end - mem::size_of::<usize>()) as *const usize), true);
    assert_eq!(h.is_ptr_inside((end - mem::size_of::<usize>() + 1) as *const usize), false);

    assert_eq!(h.safe_ref(start as *const usize).is_some(), true);
    assert_eq!(h.safe_ref((start - 1) as *const usize).is_some(), false);
    assert_eq!(h.safe_ref((end - mem::size_of::<usize>()) as *const usize).is_some(), true);
    assert_eq!(h.safe_ref((end - mem::size_of::<usize>() + 1) as *const usize).is_some(), false);
}
