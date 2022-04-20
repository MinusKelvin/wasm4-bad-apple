pub struct BitStream {
    from: &'static [u8],
    current: u8,
    current_bit: u8,
}

impl BitStream {
    pub const fn new(from: &'static [u8]) -> Self {
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

    fn read_unary(&mut self) -> Option<u32> {
        let mut v = 0;
        while !self.read_one()? {
            v += 1
        }
        Some(v)
    }

    fn read_elias_gamma(&mut self) -> Option<u32> {
        let l = self.read_unary()? as u8;
        let v = self.read_bits(l)?;
        Some(v | 1 << l)
    }

    fn read_elias_delta(&mut self) -> Option<u32> {
        let l = self.read_elias_gamma()? as u8 - 1;
        let v = self.read_bits(l)?;
        Some(v | 1 << l)
    }

    pub fn read_int(&mut self) -> Option<u32> {
        #[cfg(feature = "use-elias-gamma")]
        return self.read_elias_gamma();
        #[cfg(not(feature = "use-elias-gamma"))]
        return self.read_elias_delta();
    }
}