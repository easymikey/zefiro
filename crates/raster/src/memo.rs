#[derive(Debug)]
pub(crate) struct Memo<K, V> {
    key: Option<K>,
    remembered: Option<V>,
}

impl<K: PartialEq, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq, V> Memo<K, V> {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            key: None,
            remembered: None,
        }
    }

    pub(crate) fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> &V {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.remembered = None;
        }
        &*self.remembered.get_or_insert_with(f)
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        if self.key.as_ref() == Some(key) {
            self.remembered.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn insert(&mut self, key: K, remembered: V) {
        self.key = Some(key);
        self.remembered = Some(remembered);
    }
}

#[cfg(test)]
mod tests {
    use crate::memo::Memo;

    #[test]
    fn same_key_does_not_recompute() {
        let mut c: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        for _ in 0..3 {
            c.get_or_insert_with(7, || {
                calls += 1;
                42
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(
            *c.get_or_insert_with(7, || panic!("key 7 is already cached")),
            42
        );
    }

    #[test]
    fn different_key_recomputes() {
        let mut cache: Memo<u32, u32> = Memo::new();
        let mut calls = 0;
        let mut val = |memo: &mut Memo<u32, u32>, k: u32| -> u32 {
            *memo.get_or_insert_with(k, || {
                calls += 1;
                k * 2
            })
        };
        assert_eq!(val(&mut cache, 1), 2);
        assert_eq!(val(&mut cache, 2), 4);
        assert_eq!(val(&mut cache, 2), 4);
        assert_eq!(calls, 2);
    }

    #[test]
    fn same_bucket_different_subvalue_keeps_cache() {
        let mut c: Memo<u32, &'static str> = Memo::new();
        c.get_or_insert_with(5, || "first");
        assert_eq!(*c.get_or_insert_with(5, || "second"), "first");
    }
}
