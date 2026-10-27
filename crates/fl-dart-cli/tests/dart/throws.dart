void inner() => throw StateError('thrown on purpose');

void main() {
  inner();
}
