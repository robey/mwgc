#[cfg(test)]
mod test_mwgc {
    use core::{mem, ptr};
    use mwgc::{Heap, Memory};

    static mut DATA: [u8; 256] = [0; 256];

    // used to test the GC
    struct Sample {
        p: *const Sample,
        number: usize,
        next: *const Sample,
        prev: *const Sample,
    }

    impl Sample {
        pub fn ptr(&self) -> *const u8 {
            self as *const Sample as *const u8
        }
    }

    impl Default for Sample {
        fn default() -> Self { Sample { p: ptr::null(), number: 0, next: ptr::null(), prev: ptr::null() } }
    }


    #[test]
    fn new_heap() {
        let mut data: [u8; 256] = [0; 256];
        let start = &data[0] as *const u8;
        let h = Heap::new(Memory::new(&mut data));
        assert_eq!(h.start, start);
        assert_eq!(h.end, unsafe { h.start.offset(240) });
        assert_eq!(h.dump(), "FREE[240]");
    }

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::new(Memory::new(&mut data));
        let alloc = h.allocate(32);
        assert!(alloc.is_some());
        if let Some(m) = alloc {
            assert_eq!(m.len(), 32);
            assert_eq!(h.dump(), "Blue[32], FREE[208]");
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
            assert_eq!(h.dump(), "Blue[48], FREE[192]");
        }
    }

    #[test]
    fn retire() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::new(Memory::new(&mut data));
        let m1 = h.allocate(32).unwrap();
        let m2 = h.allocate(32).unwrap();
        h.retire(m1);
        assert_eq!(h.dump(), "FREE[32], Blue[32], FREE[176]");
        h.retire(m2);
        assert_eq!(h.dump(), "FREE[240]");

        let m3 = h.allocate_object::<Sample>().unwrap();
        assert_eq!(h.dump(), format!("Blue[{}], FREE[{}]", mem::size_of::<Sample>(), 240 - mem::size_of::<Sample>()));
        h.retire_object(m3);
        assert_eq!(h.dump(), "FREE[240]");
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
        assert_eq!(h.dump_spans(), "Blue, Blue, Blue, Blue, Blue, FREE");

        // leave o3 stranded. make o1 point to o2, which points to o4, o5, and back to o1.
        o1.p = o2 as *const Sample;
        o2.p = o4 as *const Sample;
        o2.next = o5 as *const Sample;
        o2.prev = o1 as *const Sample;
        o4.p = 455 as *const Sample;
        o5.number = 23;

        h.mark_start(&[ o1 ]);
        assert_eq!(h.get_mark_range(), (o1.ptr(), o1.ptr()));
        assert_eq!(h.dump_spans(), "Check, Blue, Blue, Blue, Blue, FREE");

        assert!(!h.mark_round());
        assert_eq!(h.get_mark_range(), (o2.ptr(), o2.ptr()));
        assert_eq!(h.dump_spans(), "Green, Check, Blue, Blue, Blue, FREE");

        assert!(!h.mark_round());
        assert_eq!(h.get_mark_range(), (o4.ptr(), o5.ptr()));
        assert_eq!(h.dump_spans(), "Green, Green, Blue, Check, Check, FREE");

        assert!(h.mark_round());
        assert_eq!(h.get_mark_range(), (core::ptr::null(), core::ptr::null()));
        assert_eq!(h.dump_spans(), "Green, Green, Blue, Green, Green, FREE");
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
        assert_eq!(h.dump_spans(), "Blue, Blue, Blue, Blue, Blue, FREE");

        o1.p = o3 as *const Sample;
        h.mark(&[ o1 ]);
        assert_eq!(h.dump_spans(), "Green, Blue, Green, Blue, Blue, FREE");
        h.sweep();
        assert_eq!(h.dump_spans(), "Green, FREE, Green, FREE");

        o1.p = core::ptr::null();
        h.mark(&[ o1 ]);
        assert_eq!(h.dump_spans(), "Blue, FREE, Green, FREE");
        h.sweep();
        assert_eq!(h.dump_spans(), "Blue, FREE");
    }

    #[test]
    fn api() {
        let mut h = Heap::new(Memory::new(unsafe { &mut DATA }));
        let o1 = h.allocate_object::<Sample>().unwrap();
        let _o2 = h.allocate_object::<Sample>().unwrap();
        let o3 = h.allocate_object::<Sample>().unwrap();
        let o4 = h.allocate_object::<Sample>().unwrap();
        let _o5 = h.allocate_object::<Sample>().unwrap();
        o1.p = o3 as *const Sample;
        o3.number = (o4.ptr() as usize) + 1;
        let stats = h.get_stats();
        assert_eq!(stats.total_bytes, 240);
        assert_eq!(stats.free_bytes, 240 - 5 * mem::size_of::<Sample>());

        h.gc(&[ o1 ]);
        assert_eq!(h.dump_spans(), "Green, FREE, Green, FREE");
        let stats2 = h.get_stats();
        assert_eq!(stats2.total_bytes, 240);
        assert_eq!(stats2.free_bytes, 240 - 2 * mem::size_of::<Sample>());
    }
}
