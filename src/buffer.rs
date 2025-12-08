pub struct Buffer<const N: usize> {
    values: [f32; N],
}

impl<const N: usize> Buffer<N> {
    pub fn new() -> Self {
        Buffer {
            values: [0.0; N]
        }
    }

    pub fn zero(&mut self) {
        self.values = [0.0; N];
    }

    pub fn average(&self) -> f32 {
        self.values.iter().sum::<f32>() / (N as f32)
    }

    /// Discards the lowest and highest value and then averages what's left.
    pub fn corrected_average(&self) -> f32 {
        let highest: f32 = self.values.iter().fold(0.0, |acc, x|  if *x > acc { *x } else { acc });
        let lowest = self.values.iter().fold(f32::MAX, |acc, x|  if *x < acc { *x } else { acc });
        let total: f32 = self.values.iter().sum::<f32>() - (highest + lowest);
        total / ((N - 2) as f32)
    }

    /// Returns the median of the buffer (middle value when sorted).
    pub fn median(&self) -> f32 {
        let mut sorted = self.values;
        // Simple insertion sort (fine for small buffers)
        for i in 1..N {
            let key = sorted[i];
            let mut j = i;
            while j > 0 && sorted[j - 1] > key {
                sorted[j] = sorted[j - 1];
                j -= 1;
            }
            sorted[j] = key;
        }
        sorted[N / 2]
    }

    pub fn push(&mut self, new: f32) {
        self.values.rotate_right(1);
        self.values[0] = new;
    }
}
