@0xf41b2e3e0df1620d;

using Rust = import "lib/rust.capnp";

$Rust.parentModule("capnp");

struct Chunk @0xeb1770b3ac9e2969 {
  pages @0 :List(Page);
}

struct Page @0xb7be16a98c43512e {
  nsId @0 :Int64;
  id @1 :UInt64;
  title @2 :Text;
  revision @3 :Revision;
}

struct Revision @0x8498a9bbaf5857f4 {
  id @0 :UInt64;
  parentId :union {
    none @2 :Void;
    some @3 :UInt64;
  }
  timestamp :union {
    none @4 :Void;
    some :group {
      # MediaWiki dumps store page.revision.timestamp as a RFC 3339 string,
      # but in practice there is no sub-second value and the time zone is always UTC
      # (so an example is '2004-01-30T01:57:23Z').
      #
      # Accordingly, this struct only stores UTC timestamp seconds, because
      # the timestamp's subsecond nanos and timezone offset seconds would just be zero
      # and a waste of space. If in the future the WikiMedia dumps contain
      # non-zero values here, we can add more fields to this group.
      utcTimestampSecs @5 :Int64;
    }
  }
  text @1 :Text;
  sha1 :union {
    none @6 :Void;
    some :group {
      # Each hash* field is a portion of the 20 byte SHA1 hash,
      # The hash is encoded as a big-endian value, with each
      # field named with the offset of the portion in the full hash value.
      hash0 @7 :UInt64;
      hash8 @8 :UInt64;
      hash16 @9 :UInt32;
    }
  }
}
