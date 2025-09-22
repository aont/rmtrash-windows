# rmtrash-windows

Windows のごみ箱を操作するためのコマンドラインツールです。以下の機能を提供します。

- 指定したファイルやフォルダーをごみ箱へ移動
- ごみ箱の容量とアイテム数の確認
- ごみ箱を空にする

> **注意:** Windows 専用ツールです。他の OS で実行した場合はエラーになります。

## インストール

```
pip install git+https://github.com/aont/rmtrash-windows.git
```

インストール後、`rmtrash` コマンドが使用できるようになります。

## 使い方

```
rmtrash [PATH ...]
rmtrash --status
rmtrash --empty [--confirm-empty] [--no-sound]
```

詳細な背景や補足情報は `0 Managing the Windows Recycle Bin.md` を参照してください。
