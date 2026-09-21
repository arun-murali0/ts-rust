interface Box {
  value: number;
}

function readLiteral(box: Box, key: "value"): number {
  return box[key];
}
