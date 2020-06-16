extern crate prusti_contracts;

// ignore-test This test fails on Silicon, but succeeds on Carbon.

// TODO: forall incorrectly encoded: 'î' is encoded as 'self'
// #[invariant="forall i: usize :: (0 <= i && i < 64) ==> self.ones[i] == 1"]
struct Ones {
    ones: [usize; 64],
}

#[ensures="result == 64"]
fn len(ones: &Ones) -> usize {
    ones.ones.len()
}

#[requires="forall i: usize :: (0 <= i && i < 64) ==> ones.ones[i] == 1"]
#[ensures="result == 2"]
fn use_ones(ones: &Ones) -> usize {
    ones.ones[1] + ones.ones[3]
}

#[trusted]
fn main() {}