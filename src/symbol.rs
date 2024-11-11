/*!

Definitions related to symbols. Symbols can be thought of as names to which additional information is attached, such as
arity and theory axioms.

In an expression like, `f[x, Thing1, 45]`, the symbols are `f`, `x`, and `Thing1`. There is also an implicit symbol
shared by all data constants, the number `45` in this case, which is defined by the client code that defined the
`DataAtom` type. Integers might be represented by the `IntegerAtom` type (implementing the `DataAtom` trait) and have
the symbol `Integer` for example.

*/

use std::fmt::Display;

use enumflags2::{bitflags, make_bitflags, BitFlags};

use crate::abstractions::IString;


pub type SymbolPtr = *const Symbol;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Symbol {
  pub name       : IString,

  pub arity      : u8,
  pub attributes : SymbolAttributes,
  pub symbol_type: SymbolType,

  // ToDo: Can the `IString` value be used as the `hash_value`?
  // Unique integer for comparing symbols, also called order.
  // In Maude, the `order` has lower bits equal to the value of an integer that is incremented every time a symbol is
  // created and upper 8 bits (bits 24..32) equal to the arity.
  pub hash_value : u32,
}

impl Symbol {
  pub fn new(name: IString, arity: u8) -> Symbol {
    let mut symbol = Symbol{
      name,
      arity,
      attributes: SymbolAttributes::default(),
      symbol_type: SymbolType::default(),
      hash_value: 0,
    };
    symbol.compute_hash();
    symbol
  }


  #[inline(always)]
  pub fn is_variable(&self) -> bool {
    self.symbol_type == SymbolType::Variable
  }
  
  fn compute_hash(&mut self) -> u32 {
    // In Maude, the hash value is the number (chronological order of creation) of the symbol OR'ed
    // with (arity << 24). Here we swap the "number" with the hash of the IString as defined by the
    // IString implementation.
    
    // ToDo: This… isn't great, because the hash is 32 bits, not 24, and isn't generated in numeric
    //       order. However, it still produces a total order on symbols in which symbols are ordered first
    //       by arity and then arbitrarily (by hash). Ordering by insertion order is just as arbitrary, so
    //       it should be ok.
    let hash = IString::get_hash(&self.name) | ((self.arity as u32) << 24); // Maude: self.arity << 24
    self.hash_value = hash;
    hash
  }
  
  /// Comparison based only on name and arity
  pub fn compare(&self, other: &Symbol) -> std::cmp::Ordering {
    self.hash_value.cmp(&other.hash_value)
  }
}

impl Display for Symbol {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    // match self.arity {
    //   Arity::Variadic => write!(f, "{}ᵥ", self.name),
    //   Arity::Value(arity) if arity > 0 => write!(f, "{}/{}", self.name, arity)
    //   _ => write!(f, "{}", self.name),
    // }
    write!(f, "{}", self.name)
  }
}

#[derive(Copy, Clone, Eq, PartialEq, Default, Debug, Hash)]
pub enum SymbolType {
  #[default]
  Standard,
  Variable,
  Operator,
  Data
}


#[bitflags]
#[repr(u32)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SymbolAttribute {
  // Syntactic attributes
  Precedence,
  Gather,
  Format,
  Latex,

  // Semantic attributes
  Strategy,
  Memoized,
  Frozen,
  Constructor,

  // Theory attributes
  Associative,
  Commutative,
  LeftIdentity,
  RightIdentity,
  Idempotent,
  Iterated,
}

pub type SymbolAttributes = BitFlags<SymbolAttribute, u32>;

impl SymbolAttribute {
  //	Conjunctions
  #![allow(non_upper_case_globals)]

  /// Theory Axioms
  pub const Axioms: SymbolAttributes = make_bitflags!(
    SymbolAttribute::{
      Associative
      | Commutative
      | LeftIdentity
      | RightIdentity
      | Idempotent
    }
  );

  pub const Collapse: SymbolAttributes = make_bitflags!(
    SymbolAttribute::{
      LeftIdentity
      | RightIdentity
      | Idempotent
    }
  );

  ///	Simple attributes are just a flag without additional data. They produce a warning if given twice.
  pub const SimpleAttributes: SymbolAttributes = make_bitflags!(
    SymbolAttribute::{
      Associative
      | Commutative
      | Idempotent
      | Memoized
      | Constructor
      | Iterated
    }
  );

  /// All flagged attributes. They need to agree between declarations of an
  /// operator.
  pub const Attributes: SymbolAttributes = make_bitflags!(
    SymbolAttribute::{
      Precedence
      | Gather
      | Format
      | Latex
      | Strategy
      | Memoized
      | Frozen
      | Associative
      | Commutative
      | LeftIdentity
      | RightIdentity
      | Idempotent
      | Iterated
    }
  );
}


