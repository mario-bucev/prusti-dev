extern crate prusti_contracts;

#[derive(Copy, Clone)]
struct Foo {
    value: usize,
    bar: Bar,
}

#[derive(Copy, Clone)]
struct Bar {
    value: usize,
}

// We cannot assert anything about the content, unfortunately. 
fn copy_from_slice(a: &mut [usize; 64], b: &[usize]) {
    a.copy_from_slice(b);
}

fn copy_from_slice_2(a: &mut [Foo; 64], b: &[Foo]) {
    a.copy_from_slice(b);
}

// Using '.len()' in contract
#[ensures="a.len() == 64"]
fn len_and_size_1(a: &[usize; 64]) {}

// Using '.len()' in method body
#[ensures="result == 64"]
fn len_and_size_2(a: &[usize; 64]) -> usize {
    a.len()
}

#[trusted]
fn main() {}