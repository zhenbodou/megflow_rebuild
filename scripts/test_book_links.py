#!/usr/bin/env python3
"""验证校验器能拒绝坏构建，而非只在当前书上输出成功。"""
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from check_book_links import check


class LinksTest(unittest.TestCase):
    def test_empty_build_is_an_error(self):
        with TemporaryDirectory() as directory:
            count, errors = check(Path(directory).resolve(), '/book/')
            self.assertEqual(count, 0)
            self.assertTrue(errors)

    def test_links_anchors_resources_and_site_prefix(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'nested').mkdir()
            (root / 'nested/index.html').write_text('<h1 id="你好">标题</h1>')
            (root / 'asset.js').write_text('')
            (root / 'index.html').write_text('''
<a href="nested/#%E4%BD%A0%E5%A5%BD">相对链接</a>
<a href="/book/nested/index.html?x=1#你好">站点前缀</a>
<script src="asset.js"></script>
<a href="https://example.com/missing">不检查网络</a>
''')
            self.assertEqual(check(root, '/book/'), (2, []))
            (root / 'index.html').write_text('''
<a href="nested/#missing">坏锚点</a>
<img src="missing.png">
<link rel="stylesheet" href="missing.css">
<a href="../outside.html">超出目录</a>
<a href="/wrong/index.html">错误站点前缀</a>
''')
            count, errors = check(root, '/book/')
            self.assertEqual(count, 2)
            self.assertEqual(len(errors), 5)
            self.assertTrue(any('锚点不存在' in error for error in errors))


if __name__ == '__main__':
    unittest.main()
