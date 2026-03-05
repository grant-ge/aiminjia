
import pickle as _pkl
import os as _os

_snap_dir = _ANALYSIS_DIR if '_ANALYSIS_DIR' in dir() else _os.path.join(_os.getcwd(), 'analysis', _CONV_ID if '_CONV_ID' in dir() else 'unknown')
_os.makedirs(_snap_dir, exist_ok=True)

# Save working DataFrame
if '_df' in dir() and hasattr(_df, 'to_pickle'):
    _pkl.dump(_df, open(_os.path.join(_snap_dir, '_step_df.pkl.tmp'), 'wb'))
    _os.replace(_os.path.join(_snap_dir, '_step_df.pkl.tmp'),
                _os.path.join(_snap_dir, '_step_df.pkl'))

# Save _dfs dict
if '_dfs' in dir() and isinstance(_dfs, dict):
    _pkl.dump(_dfs, open(_os.path.join(_snap_dir, '_step_dfs.pkl.tmp'), 'wb'))
    _os.replace(_os.path.join(_snap_dir, '_step_dfs.pkl.tmp'),
                _os.path.join(_snap_dir, '_step_dfs.pkl'))

# Save user variables
_SYS_VARS = {
    '_df', '_dfs', '_df_raw', '_CONV_ID', '_ANALYSIS_DIR', '_CURRENT_STEP',
    '_pkl', '_os', '_snap_dir', '_SYS_VARS', '_user_vars',
    '_vname', '_vval', '_ALLOWED_PATHS', '_written_files',
    '_repl_loop', '_capture', '_old_stdout',
}
_user_vars = {}
for _vname, _vval in list(globals().items()):
    if _vname.startswith('__'):
        continue
    if _vname in _SYS_VARS:
        continue
    if callable(_vval) or isinstance(_vval, type) or type(_vval).__name__ == 'module':
        continue
    try:
        _pkl.dumps(_vval)
        _user_vars[_vname] = _vval
    except Exception:
        pass
if _user_vars:
    _pkl.dump(_user_vars, open(_os.path.join(_snap_dir, '_user_vars.pkl.tmp'), 'wb'))
    _os.replace(_os.path.join(_snap_dir, '_user_vars.pkl.tmp'),
                _os.path.join(_snap_dir, '_user_vars.pkl'))
