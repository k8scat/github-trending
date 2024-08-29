# go-trending

Rust 实现，将 GitHub Trending 的开源项目推送到一些内容平台，比如知识星球，同时结合 OpenAI 自动生成项目的介绍。

## 内容平台

- [x] 知识星球

其它平台可以提 [Issue](https://github.com/k8scat/go-trending/issues) 或者 [PR](https://github.com/k8scat/go-trending/pulls)。

## 配置说明

参考 `config.example.toml` 文件进行配置，有些配置是在环境变量中设置的：

```yaml
- TRENDING_LANGUAGE=go
- OPENAI_API_BASE=https://api.openai-all.com
- OPENAI_API_KEY=sk-xxx
- OPENAI_MODEL=gpt-4o
```

## 运行

使用 Docker Compose 可以快速将该项目部署到生产环境，可以参考 `docker-compose.example.yml` 文件进行配置。

## 交流群

知识星球：[Rust 开发笔记](https://t.zsxq.com/4bVnF)

## 开源协议

[MIT](./LICENSE)
