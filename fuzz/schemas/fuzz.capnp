@0xeb3e36f1c0cffd7c;

enum TestEnum {
  foo @0;
  bar @1;
  baz @2;
  qux @3;
  quux @4;
  corge @5;
  grault @6;
  garply @7;
}

struct TestAllTypes {
  voidField      @0  : Void;
  boolField      @1  : Bool;
  int8Field      @2  : Int8;
  int16Field     @3  : Int16;
  int32Field     @4  : Int32;
  int64Field     @5  : Int64;
  uint8Field     @6  : UInt8;
  uint16Field    @7  : UInt16;
  uint32Field    @8  : UInt32;
  uint64Field    @9  : UInt64;
  float32Field   @10 : Float32;
  float64Field   @11 : Float64;
  textField      @12 : Text;
  dataField      @13 : Data;
  structField    @14 : TestAllTypes;
  enumField      @15 : TestEnum;

  voidList      @16 : List(Void);
  boolList      @17 : List(Bool);
  int8List      @18 : List(Int8);
  int16List     @19 : List(Int16);
  int32List     @20 : List(Int32);
  int64List     @21 : List(Int64);
  uint8List     @22 : List(UInt8);
  uint16List    @23 : List(UInt16);
  uint32List    @24 : List(UInt32);
  uint64List    @25 : List(UInt64);
  float32List   @26 : List(Float32);
  float64List   @27 : List(Float64);
  textList      @28 : List(Text);
  dataList      @29 : List(Data);
  structList    @30 : List(TestAllTypes);
  enumList      @31 : List(TestEnum);
}
