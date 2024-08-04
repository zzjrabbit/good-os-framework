use bit_field::BitField;

pub struct Bitmap {
    inner: &'static mut [u8],
}

impl Bitmap {
    pub fn new(inner: &'static mut [u8]) -> Self {
        inner.fill(0);
        Self { inner }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn get(&self, index: usize) -> bool {
        let byte = self.inner[index / 8];
        byte.get_bit(index % 8)
    }

    pub fn set(&mut self, index: usize, value: bool) {
        let byte = &mut self.inner[index / 8];
        byte.set_bit(index % 8, value);
    }
}
