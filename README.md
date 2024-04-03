# listup_jorei

日本全国の条例の情報を<https://jorei.slis.doshisha.ac.jp/>をもとにローカルに保存するスクリプトです。


## CLIソフトウェアを使う

### インストール

```sh
cargo install --git "https://github.com/japanese-law-analysis/listup_jorei.git"
```

### 使い方

```sh
listup_jorei -o output
```

で起動します。

オプションの各意味は以下のとおりです。

- `-o`：解析で生成した情報を出力するフォルダ

---

[MIT License](https://github.com/japanese-law-analysis/listup_jorei/blob/master/LICENSE)
(c) 2024 Naoki Kaneko (a.k.a. "puripuri2100")

