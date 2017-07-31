#!/bin/bash

export PATH="$PYENV_ROOT/bin:$PATH"
. $HOME/.cargo/env
eval "$(pyenv init -)"

exec "$@"
