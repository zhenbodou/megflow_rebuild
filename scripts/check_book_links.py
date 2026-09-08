#!/usr/bin/env python3
"""检查生成 HTML 的本地链接、锚点和直接引用资源；不请求外部网站。"""
import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.references = []
        self.ids = set()

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        if tag == 'a' and 'name' in attrs:
            self.ids.add(attrs['name'])
        attribute = {'a': 'href', 'link': 'href', 'img': 'src', 'script': 'src'}.get(tag)
        if attribute and attribute in attrs:
            self.references.append(attrs[attribute])


def check(root, site_path):
    pages = {}
    for path in root.rglob('*.html'):
        page = Page()
        page.feed(path.read_text(encoding='utf-8'))
        pages[path.resolve()] = page
    if not pages:
        return 0, ['没有生成的 HTML，请先执行 mdbook build book。']
    errors = []
    for path, page in pages.items():
        for reference in page.references:
            url = urlsplit(reference)
            if url.scheme or url.netloc:
                continue
            target_path = unquote(url.path)
            if target_path.startswith('/'):
                if not target_path.startswith(site_path):
                    errors.append(f'{path.relative_to(root)}: {reference} → 超出站点前缀 {site_path}')
                    continue
                target = root / target_path[len(site_path):]
            else:
                target = path.parent / target_path if target_path else path
            target = target.resolve()
            if target.is_dir():
                target = target / 'index.html'
            reason = None
            if not target.is_relative_to(root):
                reason = '超出构建目录'
            elif not target.is_file():
                reason = '文件不存在'
            elif target in pages and url.fragment and unquote(url.fragment) not in pages[target].ids:
                reason = '锚点不存在'
            if reason:
                errors.append(f'{path.relative_to(root)}: {reference} → {reason}')
    return len(pages), errors


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=repo / 'book/book')
    parser.add_argument('--site-path', default='/megflow_rebuild/')
    args = parser.parse_args()
    prefix = '/' + args.site_path.strip('/') + '/' if args.site_path.strip('/') else '/'
    count, errors = check(args.root.resolve(), prefix)
    print(f'HTML pages: {count}; broken references: {len(errors)}')
    for error in errors:
        print(error)
    return bool(errors)


if __name__ == '__main__':
    raise SystemExit(main())
