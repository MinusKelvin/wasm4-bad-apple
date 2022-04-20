use std::io::Write;

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

    fn write_unary(&mut self, v: u32) {
        for _ in 0..v {
            self.write(false);
        }
        self.write(true);
    }

    fn write_elias_gamma(&mut self, v: u32) {
        assert_ne!(v, 0);
        let n = (v + 1).next_power_of_two().trailing_zeros() - 1;
        self.write_unary(n);
        self.write_bits(v, n);
    }

    fn write_elias_delta(&mut self, v: u32) {
        assert_ne!(v, 0);
        let n = (v + 1).next_power_of_two().trailing_zeros() - 1;
        self.write_elias_gamma(n + 1);
        self.write_bits(v, n);
    }

    pub fn write_int(&mut self, v: u32) {
        #[cfg(feature = "use-elias-gamma")]
        self.write_elias_gamma(v);
        #[cfg(not(feature = "use-elias-gamma"))]
        self.write_elias_delta(v);
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
