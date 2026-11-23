# Simulates the util.rs random_bytes FALLBACK arithmetic (the Windows path,
# /dev/urandom absent) under the worst case the Windows CI hit: two calls in
# the SAME millisecond, same pid, and the heap re-serving the SAME address
# for the same-size block.
# Old seed: now_ms ^ pid*G ^ addr          (no counter -> identical streams)
# New seed:  now_ms ^ pid*G ^ ctr*G.rot32 ^ addr (counter differs per call)
GOLDEN = 0x9E3779B97F4A7C15
MASK = (1 << 64) - 1


def old_state(now_ms, pid, addr):
    return (now_ms ^ ((pid * GOLDEN) & MASK) ^ addr) & MASK


def new_state(now_ms, pid, addr, ctr):
    ctr_term = ((ctr * GOLDEN) & MASK)
    ctr_term = ((ctr_term << 32) | (ctr_term >> 32)) & MASK  # rotate_left(32)
    return (now_ms ^ ((pid * GOLDEN) & MASK) ^ ctr_term ^ addr) & MASK


def stream(state, n=16):
    out = []
    for _ in range(n):
        state ^= (state << 13) & MASK
        state &= MASK
        state ^= state >> 7
        state ^= (state << 17) & MASK
        state &= MASK
        out.append((state >> 24) & 0xFF)
    return bytes(out)


NOW, PID, ADDR = 1_700_000_000_000, 4242, 0x7FFDEAD_BEEF  # identical for both calls

# Old behavior: ctr absent -> same seed -> same stream -> token-secret collision
a_old, b_old = stream(old_state(NOW, PID, ADDR)), stream(old_state(NOW, PID, ADDR))
# New behavior: ctr 0 vs 1 (same ms/pid/addr recycled)
a_new = stream(new_state(NOW, PID, ADDR, 0))
b_new = stream(new_state(NOW, PID, ADDR, 1))

print(f"old: identical streams? {a_old == b_new and False or a_old == b_old}")
print(f"new: identical streams? {a_new == b_new}")
assert a_old == b_old, "old fallback unexpectedly distinct"
assert a_new != b_new, "new fallback collided — fix ineffective"
print("PROOF: old fallback collided under the Windows worst case; the counter fix keeps rapid calls distinct.")
