pub struct BitStream<'a> {
    from: &'a [u8],
    current: u8,
    current_bit: u8,
}

impl BitStream<'_> {
    pub const fn new(from: &[u8]) -> BitStream {
        BitStream {
            from,
            current: 0,
            current_bit: 8,
        }
    }

    pub fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut bits = 0;
        for i in 0..count {
            bits |= (self.read_one()? as u32) << i;
        }
        Some(bits)
    }

    pub fn read_one(&mut self) -> Option<bool> {
        if self.current_bit == 8 {
            let (&next, rest) = self.from.split_first()?;
            self.current = next;
            self.from = rest;
            self.current_bit = 0;
        }
        let result = self.current & 1 << self.current_bit != 0;
        self.current_bit += 1;
        Some(result)
    }

    fn read_fibonacci(&mut self) -> Option<u32> {
        let mut v = 0;
        let mut a = 1;
        let mut b = 2;
        let mut prev = false;
        loop {
            let next = self.read_one()?;
            if next {
                if prev {
                    return Some(v);
                }
                v += a;
            }
            let c = a + b;
            a = b;
            b = c;
            prev = next;
        }
    }

    pub fn read_int(&mut self) -> Option<u32> {
        self.read_fibonacci()
    }
}