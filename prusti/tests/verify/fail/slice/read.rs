extern crate prusti_contracts;

// ignore-test

fn return_fixed(arr: &[isize]) -> isize {
    return arr[0]; //~ ERROR assertion might fail with "array index out of bounds"
}

// We need i >= 0 because unsigned integers bounds are not encoded by default
#[requires="0 <= i && i < arr.len()"]
fn return_nth(arr: &[isize], i: usize) -> isize {
    return arr[i + 1]; //~ ERROR assertion might fail with "array index out of bounds"
}

fn sum_all(arr: &[isize]) -> isize {
    let mut sum = 0;
    let mut i = 0;
    while 0 <= i {
        sum += arr[i]; //~ ERROR assertion might fail with "array index out of bounds"
        i += 1;
    }
    return sum;
}

fn sum_alternate(arr: &[isize]) -> isize {
    let mut sum = 0;
    let mut i = 0;
    while 0 <= i && i <= arr.len() {
        sum += arr[i]; //~ ERROR assertion might fail with "array index out of bounds"
        i += 2;
    }
    return sum;
}

fn main() {}