use std::io::Write;

const FIBONACCI: [u32; 46] = {
    let mut numbers = [0; 46];
    numbers[0] = 1;
    numbers[1] = 2;
    let mut i = 2;
    while i < numbers.len() {
        numbers[i] = numbers[i - 2] + numbers[i - 1];
        i += 1;
    }
    numbers
};

pub struct BitVec {
    data: Vec<bool>, // lol
}

impl BitVec {
    pub fn new() -> Self {
        BitVec { data: vec![] }
    }

    pub fn write(&mut self, v: bool) {
        self.data.push(v);
    }

    pub fn write_bits(&mut self, v: u32, count: u32) {
        for i in 0..count {
            self.write(v & 1 << i != 0);
        }
    }

    fn write_fibonacci(&mut self, mut v: u32) {
        assert_ne!(v, 0);
        let start = self.data.len();
        let i = match FIBONACCI.binary_search(&v) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        for &num in FIBONACCI[..i].iter().rev() {
            self.write(v >= num);
            if v >= num {
                v -= num;
            }
        }
        self.data[start..].reverse();
        // terminator
        self.write(true);
    }

    pub fn write_int(&mut self, v: u32) {
        self.write_fibonacci(v);
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn append(&mut self, other: BitVec) {
        self.data.extend(other.data);
    }

    pub fn dump(&self, mut to: impl Write) -> std::io::Result<()> {
        for vs in self.data.chunks(8) {
            to.write_all(&[vs
                .iter()
                .enumerate()
                .map(|(i, &v)| (v as u8) << i)
                .reduce(|a, b| a | b)
                .unwrap()])?;
        }
        Ok(())
    }
}
