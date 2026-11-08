# SPDX-License-Identifier: GPL-3.0-or-later
"""環境の事実を作るアダプタ。プラットフォームごとに 1 つ。plan や実環境を読んで、中立な事実（geas/envcheck.py の FACTS）の JSON にする。
読めなかった事実は null にする（言い切れないときに、合格になる方へ倒さない）。"""
from . import aws_ecs

ADAPTERS = {"aws-ecs": aws_ecs}
