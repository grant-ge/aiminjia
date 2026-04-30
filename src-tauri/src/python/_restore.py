
import pickle as _pkl
import os as _os

_snap_dir = _ANALYSIS_DIR if '_ANALYSIS_DIR' in dir() else None
if _snap_dir is None:
    # Try to infer from workspace
    _analysis_base = _os.path.join(_os.getcwd(), 'analysis')
    if _os.path.exists(_analysis_base):
        _convs = [d for d in _os.listdir(_analysis_base) if _os.path.isdir(_os.path.join(_analysis_base, d))]
        if _convs:
            _snap_dir = _os.path.join(_analysis_base, sorted(_convs)[-1])

if _snap_dir and _os.path.exists(_snap_dir):
    # Restore _df
    _snap_path = _os.path.join(_snap_dir, '_step_df.pkl')
    if _os.path.exists(_snap_path):
        if _ckpt_verify(_snap_path):
            _df = _pkl.load(open(_snap_path, 'rb'))
        else:
            import sys as _sys
            print(f'[WARN] Checkpoint signature invalid or missing: {_snap_path} — skipped (possible tampering)', file=_sys.stderr)

    # Restore _dfs
    _snap_dfs = _os.path.join(_snap_dir, '_step_dfs.pkl')
    if _os.path.exists(_snap_dfs):
        if _ckpt_verify(_snap_dfs):
            _dfs = _pkl.load(open(_snap_dfs, 'rb'))
        else:
            import sys as _sys
            print(f'[WARN] Checkpoint signature invalid or missing: {_snap_dfs} — skipped (possible tampering)', file=_sys.stderr)

    # Restore user vars
    _uv_path = _os.path.join(_snap_dir, '_user_vars.pkl')
    if _os.path.exists(_uv_path):
        if _ckpt_verify(_uv_path):
            try:
                for _k, _v in _pkl.load(open(_uv_path, 'rb')).items():
                    globals()[_k] = _v
                del _k, _v
            except Exception:
                pass
        else:
            import sys as _sys
            print(f'[WARN] Checkpoint signature invalid or missing: {_uv_path} — skipped (possible tampering)', file=_sys.stderr)

    # Restore _df_raw
    _orig_path = _os.path.join(_snap_dir, '_original.pkl')
    if _os.path.exists(_orig_path):
        if _ckpt_verify(_orig_path):
            _df_raw = _pkl.load(open(_orig_path, 'rb'))
        else:
            import sys as _sys
            print(f'[WARN] Checkpoint signature invalid or missing: {_orig_path} — skipped (possible tampering)', file=_sys.stderr)
