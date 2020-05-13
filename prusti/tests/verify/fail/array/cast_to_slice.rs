extern crate prusti_contracts;

// Using '.len()' in contract
#[ensures="a.len() == 63"] //~ ERROR postcondition might not hold.
fn len_and_size_1(a: &[usize; 64]) {}

// Using '.len()' in method body
#[ensures="result == 67"] //~ ERROR postcondition might not hold.
fn len_and_size_2(a: &[usize; 64]) -> usize {
    a.len()
}

#[trusted]
fn main() {}