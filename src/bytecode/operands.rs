//! Storage indexed by bytecode operand: arguments, then locals, then tmps.
//!
//! Faithful port of C++ `bytecode/Operands.h` (`class Operands<T>`,
//! Operands.h:138): one flat buffer that stores the arguments first, then the
//! locals, then the tmps, with typed index accessors. The DFG uses
//! `Operands<Node*>` for per-block variable state (`variablesAtHead` /
//! `variablesAtTail`, dfg/DFGBasicBlock.h:216-217); the Rust port uses
//! `Operands<Option<DfgNodeId>>` there.

/// `Operands<T>` (bytecode/Operands.h:138).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operands<T> {
    num_arguments: usize,
    num_locals: usize,
    values: Vec<T>,
}

impl<T> Default for Operands<T> {
    fn default() -> Self {
        Self {
            num_arguments: 0,
            num_locals: 0,
            values: Vec::new(),
        }
    }
}

impl<T: Clone + Default> Operands<T> {
    /// `Operands(numArguments, numLocals, numTmps)` filling with `T()`
    /// (Operands.h:148-156).
    pub fn new(num_arguments: usize, num_locals: usize, num_tmps: usize) -> Self {
        Self {
            num_arguments,
            num_locals,
            values: vec![T::default(); num_arguments + num_locals + num_tmps],
        }
    }
}

impl<T> Operands<T> {
    /// (Operands.h:180)
    pub fn number_of_arguments(&self) -> usize {
        self.num_arguments
    }

    /// (Operands.h:181)
    pub fn number_of_locals(&self) -> usize {
        self.num_locals
    }

    /// (Operands.h:182)
    pub fn number_of_tmps(&self) -> usize {
        self.values.len() - self.num_arguments - self.num_locals
    }

    pub fn size(&self) -> usize {
        self.values.len()
    }

    /// `argumentIndex` (Operands.h:190-193): arguments occupy the front of the
    /// buffer.
    fn argument_index(&self, idx: usize) -> usize {
        assert!(idx < self.num_arguments);
        idx
    }

    /// `localIndex` (Operands.h:195-199): locals follow the arguments.
    fn local_index(&self, idx: usize) -> usize {
        assert!(idx < self.num_locals);
        self.num_arguments + idx
    }

    /// `tmpIndex` (Operands.h:184-188): tmps follow arguments and locals.
    fn tmp_index(&self, idx: usize) -> usize {
        assert!(idx < self.number_of_tmps());
        self.num_arguments + self.num_locals + idx
    }

    /// (Operands.h:204-205)
    pub fn argument(&self, idx: usize) -> &T {
        &self.values[self.argument_index(idx)]
    }

    pub fn argument_mut(&mut self, idx: usize) -> &mut T {
        let index = self.argument_index(idx);
        &mut self.values[index]
    }

    /// (Operands.h:207-208)
    pub fn local(&self, idx: usize) -> &T {
        &self.values[self.local_index(idx)]
    }

    pub fn local_mut(&mut self, idx: usize) -> &mut T {
        let index = self.local_index(idx);
        &mut self.values[index]
    }

    /// (Operands.h:201-202)
    pub fn tmp(&self, idx: usize) -> &T {
        &self.values[self.tmp_index(idx)]
    }

    pub fn tmp_mut(&mut self, idx: usize) -> &mut T {
        let index = self.tmp_index(idx);
        &mut self.values[index]
    }

    /// Raw linear access (`at`/`operator[]` in Operands.h).
    pub fn at(&self, index: usize) -> &T {
        &self.values[index]
    }

    pub fn at_mut(&mut self, index: usize) -> &mut T {
        &mut self.values[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operands_lays_out_arguments_then_locals_then_tmps() {
        let mut operands: Operands<Option<u32>> = Operands::new(2, 3, 1);
        assert_eq!(operands.number_of_arguments(), 2);
        assert_eq!(operands.number_of_locals(), 3);
        assert_eq!(operands.number_of_tmps(), 1);
        assert_eq!(operands.size(), 6);

        *operands.argument_mut(1) = Some(10);
        *operands.local_mut(0) = Some(20);
        *operands.tmp_mut(0) = Some(30);

        // Flat layout: arguments occupy [0, numArguments), locals follow, tmps
        // last (Operands.h:184-199).
        assert_eq!(operands.at(1), &Some(10));
        assert_eq!(operands.at(2), &Some(20));
        assert_eq!(operands.at(5), &Some(30));
        assert_eq!(operands.argument(0), &None);
        assert_eq!(operands.local(2), &None);
    }
}
