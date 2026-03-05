
import sys, os, io, json, time, traceback, builtins

# Will be populated by sandbox preamble injection
_written_files = []

def _repl_loop():
    while True:
        line = sys.stdin.readline()
        if not line:
            break  # stdin closed
        line = line.strip()
        if not line.startswith('__EXEC__'):
            continue

        parts = line.split(' ', 2)
        uuid = parts[1] if len(parts) > 1 else 'unknown'
        _timeout = int(parts[2]) if len(parts) > 2 else 120

        # Read code block until __END__
        code_lines = []
        for code_line in sys.stdin:
            if code_line.strip() == '__END__':
                break
            code_lines.append(code_line)
        code = ''.join(code_lines)

        # Reset per-execution state
        _written_files.clear()
        start = time.time()
        exit_code = 0

        # Capture user stdout to StringIO
        _capture = io.StringIO()
        _old_stdout = sys.stdout
        sys.stdout = _capture

        try:
            exec(code, globals())
        except KeyboardInterrupt:
            exit_code = 130
        except SystemExit as _se:
            exit_code = _se.code if isinstance(_se.code, int) else 1
        except Exception:
            sys.stdout = _old_stdout
            traceback.print_exc()  # goes to stderr
            exit_code = 1
        finally:
            sys.stdout = _old_stdout

        elapsed = int((time.time() - start) * 1000)
        user_stdout = _capture.getvalue()

        # Write meta to temp JSON file (exit_code + timing for Rust-side send_code)
        # NOTE: __GENERATED_FILE__ markers are NOT stripped from stdout here.
        # They pass through to Rust-side handle_execute_python for uniform parsing
        # regardless of session vs one-shot runner mode.
        meta_path = os.path.join(os.getcwd(), 'temp', f'_meta_{uuid}.json')
        try:
            os.makedirs(os.path.dirname(meta_path), exist_ok=True)
            with open(meta_path, 'w', encoding='utf-8') as f:
                json.dump({
                    'exit_code': exit_code,
                    'execution_time_ms': elapsed,
                    'written_paths': list(_written_files),
                }, f, ensure_ascii=False)
        except Exception as e:
            print(f'[WARN] Failed to write meta: {e}', file=sys.stderr)

        # Output user stdout (including __GENERATED_FILE__ markers) + completion signal
        _old_stdout.write(user_stdout)
        if user_stdout and not user_stdout.endswith('\n'):
            _old_stdout.write('\n')
        _old_stdout.write(f'__DONE__ {uuid}\n')
        _old_stdout.flush()

if __name__ == '__main__':
    _repl_loop()
