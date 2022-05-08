use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::io::Write;

use crate::bitvec::BitVec;

pub struct HuffmanCode<T> {
    value_to_codeword: HashMap<T, BitVec>,
    codeword_tree: Code<T>,
}

enum Code<T> {
    Value(T),
    Split(Box<Code<T>>, Box<Code<T>>),
}

struct FrequencyCode<T> {
    freq: u64,
    count: usize,
    code: Code<T>,
}

impl<T: Hash + Eq + Clone> HuffmanCode<T> {
    pub fn new(counts: impl IntoIterator<Item = (T, u64)>) -> HuffmanCode<T> {
        let mut pqueue: BinaryHeap<FrequencyCode<T>> = counts
            .into_iter()
            .map(|(v, freq)| FrequencyCode {
                freq,
                count: 1,
                code: Code::Value(v),
            })
            .collect();

        while pqueue.len() > 1 {
            let mut next_2 = pqueue.pop().unwrap();
            let mut next_1 = pqueue.pop().unwrap();
            if next_2.count < next_1.count {
                std::mem::swap(&mut next_1, &mut next_2);
            }
            pqueue.push(FrequencyCode {
                freq: next_1.freq + next_2.freq,
                count: next_1.count + next_2.count,
                code: Code::Split(Box::new(next_1.code), Box::new(next_2.code)),
            });
        }

        let codeword_tree = pqueue.pop().unwrap().code;

        let mut value_to_codeword = HashMap::new();
        build_value_map(&mut value_to_codeword, BitVec::new(), &codeword_tree);

        HuffmanCode {
            value_to_codeword,
            codeword_tree,
        }
    }

    pub fn encode_value(&self, into: &mut BitVec, v: &T) {
        into.append(self.value_to_codeword.get(v).unwrap());
    }

    pub fn emit_decoder<W: Write>(
        &self,
        to: &mut W,
        name: &str,
        ty: &str,
        mut emit_code: impl FnMut(&mut W, &T) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        write!(to, "pub fn {name}(mut next: impl FnMut() -> bool) -> {ty} {{")?;
        emit_codeword_decoder(to, &mut emit_code, &self.codeword_tree)?;
        write!(to, "}}")
    }

    pub fn structure(&self) -> (BitVec, Vec<T>) {
        let mut tree = BitVec::new();
        let mut values = vec![];
        structure_bits(&mut tree, &mut values, &self.codeword_tree);
        (tree, values)
    }
}

fn build_value_map<T: Hash + Eq + Clone>(
    map: &mut HashMap<T, BitVec>,
    mut codeword: BitVec,
    tree: &Code<T>,
) {
    match tree {
        Code::Value(v) => {
            let old = map.insert(v.clone(), codeword);
            assert!(old.is_none());
        }
        Code::Split(zero, one) => {
            let mut z = codeword.clone();
            z.write(false);
            build_value_map(map, z, zero);
            codeword.write(true);
            build_value_map(map, codeword, one);
        }
    }
}

fn emit_codeword_decoder<T, W: Write>(
    to: &mut W,
    emit_code: &mut impl FnMut(&mut W, &T) -> std::io::Result<()>,
    code: &Code<T>,
) -> std::io::Result<()> {
    match code {
        Code::Value(v) => emit_code(to, v),
        Code::Split(zero, one) => {
            write!(to, "if next() {{")?;
            emit_codeword_decoder(to, emit_code, one)?;
            write!(to, "}} else {{")?;
            emit_codeword_decoder(to, emit_code, zero)?;
            write!(to, "}}")
        }
    }
}

fn structure_bits<T: Clone>(tree: &mut BitVec, value: &mut Vec<T>, code: &Code<T>) {
    match code {
        Code::Value(v) => {
            tree.write(true);
            value.push(v.clone());
        }
        Code::Split(zero, one) => {
            tree.write(false);
            structure_bits(tree, value, zero);
            structure_bits(tree, value, one);
        },
    }
}

impl<T> Ord for FrequencyCode<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.freq.cmp(&other.freq).reverse()
    }
}

impl<T> PartialOrd for FrequencyCode<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> PartialEq for FrequencyCode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.freq == other.freq
    }
}

impl<T> Eq for FrequencyCode<T> {}
