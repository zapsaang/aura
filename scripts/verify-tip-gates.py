import argparse
import sys

sys.dont_write_bytecode = True

from gate_worktree import GateConfig, GateError, run_gate


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", required=True)
    parser.add_argument("--rust-toolchain", required=True)
    parser.add_argument("--locked", action="store_true")
    parser.add_argument("--ploc-max", required=True, type=int)
    args = parser.parse_args()
    if not args.locked or args.rust_toolchain != "1.70.0" or args.ploc_max != 250:
        sys.stderr.write("verify-tip-gates: unsupported gate configuration\n")
        return 1
    try:
        run_gate(GateConfig(args.commit, args.rust_toolchain, "check"))
    except GateError as error:
        sys.stderr.write(f"verify-tip-gates: {error}\n")
        return 1
    sys.stdout.write('{"checks":2,"status":"ok"}\n')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
