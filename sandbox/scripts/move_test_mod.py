#!/usr/bin/env python3
"""Move the #[cfg(test)] mod tests block to the end of a Rust source file."""
import sys

path = sys.argv[1]
lines = open(path).read().split('\n')
start = next(i for i, l in enumerate(lines) if l.strip() == '#[cfg(test)]')
end = next(i for i in range(start + 1, len(lines)) if lines[i] == '}')
block = lines[start:end + 1]
rest = lines[:start] + lines[end + 1:]
while rest and rest[-1] == '':
    rest.pop()
out = rest + [''] + block + ['']
open(path, 'w').write('\n'.join(out))
print(f'moved {end - start + 1} lines (test module) to end of {path}')
