use enumflags2::{make_bitflags, BitFlags, bitflags};


#[bitflags]
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum DagNodeFlag {
  /// Marked as in use
  Marked,
  /// Has args that need destruction
  NeedsDestruction,
  /// Reduced up to strategy by equations
  Reduced,
  /// Copied in current copy operation; copyPointer valid
  Copied,
  /// Reduced and not rewritable by rules
  Unrewritable,
  /// Unrewritable and all subterms unstackable or frozen
  Unstackable,
  /// No variables occur below this node
  GroundFlag,
  /// Node has a valid hash value (storage is theory dependent)
  HashValid,
}

impl DagNodeFlag {
  #![allow(non_upper_case_globals)]

  /// An alias - We can share the same bit for this flag since the rule rewriting
  /// strategy that needs `Unrewritable` will never be combined with variant narrowing.
  pub const IrreducibleByVariantEquations: DagNodeFlag = DagNodeFlag::Unrewritable;

  // Conjunctions

  /// Flags for rewriting
  pub const RewritingFlags: DagNodeFlags = make_bitflags!(
    DagNodeFlag::{
      Reduced | Unrewritable | Unstackable | GroundFlag
    }
  );
}

pub type DagNodeFlags = BitFlags<DagNodeFlag, u8>;
