use std::ops::Range;
use mmtk::util::Address;

pub type ScalaNativeEdge = Address;

pub type ScalaNativeMemorySlice = Range<ScalaNativeEdge>;