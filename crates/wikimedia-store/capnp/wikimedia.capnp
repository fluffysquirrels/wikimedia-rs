@0xf41b2e3e0df1620d;

using Rust = import "lib/rust.capnp";

$Rust.parentModule("capnp");

struct Chunk @0xeb1770b3ac9e2969 {
  pages @0 :List(Page);
}

struct Page @0xb7be16a98c43512e {
  nsId @0 :UInt64;
  id @1 :UInt64;
  title @2 :Text;
  revision @3 :Revision;
}

struct Revision @0x8498a9bbaf5857f4 {
  id @0 :UInt64;
  text @1 :Text;
}
