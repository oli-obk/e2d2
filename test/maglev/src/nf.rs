use e2d2::headers::*;
use e2d2::packet_batch::*;
use e2d2::utils::*;
use e2d2::scheduler::*;
use fnv::FnvHasher;
use twox_hash::XxHash;
use std::net::Ipv4Addr;
use std::convert::From;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::hash::{Hash, Hasher, BuildHasher};

type FnvHash = BuildHasherDefault<FnvHasher>;
type XxHashFactory = BuildHasherDefault<XxHash>;

struct Maglev {
    //permutation: Box<Vec<Vec<usize>>>,
    lut: Box<Vec<usize>>,
    lut_size: usize,
}

impl Maglev {
    pub fn offset_skip_for_name(name: &str, h1: &FnvHash, h2: &XxHashFactory, lsize: usize) -> (usize, usize) {
        let mut fnv_state = h1.build_hasher();
        name.hash(&mut fnv_state);
        let hash1 = fnv_state.finish() as usize;
        let mut xx_state = h2.build_hasher();
        name.hash(&mut xx_state);
        let hash2 = xx_state.finish() as usize;
        let offset = hash2 % lsize;
        let skip = hash1 % (lsize - 1) + 1;
        (offset, skip)
    }

    pub fn generate_permutations(backends: &[&str], lsize: usize) -> Vec<Vec<usize>> {
        println!("Generating permutations");
        let fnv_hasher: FnvHash = Default::default();
        let xx_hasher: XxHashFactory = Default::default();
        backends.iter().map(|n| {
            Maglev::offset_skip_for_name(n, &fnv_hasher, &xx_hasher, lsize)
        }).map(|(offset, skip)| (0..lsize).map(|j| { (offset + j * skip) % lsize }).collect()).collect()
    }

    fn generate_lut(permutations: &Vec<Vec<usize>>, size: usize) -> Box<Vec<usize>> {
        let mut next:Vec<_> = permutations.iter().map(|_| 0).collect();
        let mut entry:Box<Vec<usize>> = box ((0..size).map(|_| 0x8000).collect());
        let mut n = 0;
        println!("Generating LUT");
        while n < size {
            for i in 0..next.len() {
                let mut c = permutations[i][next[i]];
                while entry[c] != 0x8000 {
                    next[i] += 1;
                    c = permutations[i][next[i]];
                };
                if entry[c] == 0x8000 {
                    entry[c] = i;
                    next[i] += 1;
                    n += 1;
                }
                if n >= size {
                    break
                }
            }
        };
        println!("Done Generating LUT");
        entry
    }

    pub fn new(name: &[&str], lsize: usize) -> Maglev {
        let permutations = box Maglev::generate_permutations(name, lsize);
        Maglev {
            lut: Maglev::generate_lut(&*permutations, lsize),
            lut_size: lsize,
        }
    }

    pub fn lookup(&self, hash: usize) -> usize {
        let idx = hash % self.lut_size;
        self.lut[idx]
    }
}

#[derive(Default, Clone)]
struct Empty;
pub fn maglev<T: 'static + Batch>(parent: T, s: &mut Scheduler, backends: &[&str]) 
            -> CompositionBatch {
    let ct = backends.len();
    let lut = Maglev::new(backends, 1433);
    let mut groups = parent.parse::<MacHeader>()
                    .group_by::<Empty>(ct, box move |hdr, payload, _| {
                        (lut.lookup(ipv4_flow_hash(hdr, payload, 0)), None)
                    }, s);
    let pipeline = merge((0..ct).map(|i| groups.get_group(i).unwrap()).collect());
    pipeline.compose()
}
