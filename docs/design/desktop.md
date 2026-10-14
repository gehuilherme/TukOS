# Design do desktop do TukOS (shell — subprojeto 7)

Direção visual fornecida pelo mantenedor em `design-desktop.pdf` (14/10/2026), com modos claro e escuro. Este documento resume as decisões de design para quando o shell/desktop for implementado (muito depois do degrau 3a). Não afeta os planos de kernel atuais.

> Clean room / marca: o layout segue convenções consagradas de desktop (barra de tarefas, menu iniciar, central de ações), mas usa ícones, cores, tipografia e nomes **próprios** do TukOS, em português e genéricos ("Arquivos", "Navegador", "Loja de apps", "Configurações"). Proibido usar a marca, os ícones ou a identidade visual da Microsoft.

## Linguagem visual
- **Estética:** flat, cantos bem arredondados (~12–16px nos cards/janelas), superfícies com leve translucidez/blur sobre o papel de parede, sombras suaves. Híbrido Fluent/Material.
- **Papel de parede:** gradiente diagonal azul → lavanda/rosa (mesmo no claro e no escuro; o escuro escurece as superfícies, não o fundo).
- **Acento:** azul (~#3B6EF5). Usado em seleção, toggles ligados, botões primários, ícone ativo na barra de tarefas (com realce/underline).
- **Ícones de app:** coloridos, cada um com sua cor (pasta amarela, navegador azul, mensagens verde, calendário vermelho, loja roxa, fotos azul-claro, música rosa, e-mail azul). Traço simples, preenchimento sólido.
- **Modo claro:** superfícies brancas/levemente azuladas, texto quase-preto, divisórias cinza-claro.
- **Modo escuro:** superfícies azul-escuro/quase-preto translúcidas, texto branco, divisórias sutis; o acento azul fica um pouco mais claro.

## Elementos (telas no PDF)
1. **Área de trabalho + barra de tarefas:** ícones no canto superior esquerdo (Este computador, Lixeira, Projetos). Barra inferior centralizada com os apps fixados; à esquerda um widget de clima (temperatura + condição), à direita a bandeja (Wi-Fi, volume, bateria) e relógio/data. O app em foco tem realce.
2. **Menu iniciar:** painel central flutuante; campo de busca ("Pesquisar apps, arquivos e configurações"); grade "Fixados" (6 por linha) + "Todos os apps ›"; seção "Recomendados" (arquivos recentes); rodapé com avatar do usuário e botão de energia.
3. **Central de ações + notificações:** à esquerda, grade de toggles rápidos (Wi-Fi, Bluetooth, Modo escuro, Não perturbe, Economia de energia, Projetar tela) + sliders de brilho e volume + status da bateria; à direita, lista de notificações ("Limpar tudo") e um mini-calendário do mês.
4. **Tela de bloqueio:** relógio grande + data no canto superior esquerdo; card central com avatar, nome, campo de senha, "Esqueci minha senha", "Alternar para outro usuário"; canto inferior direito com Wi-Fi, acessibilidade, energia.
5. **Configurações:** janela com barra lateral (Sistema, Bluetooth e dispositivos, Rede e internet, Personalização, Apps, Contas, Hora e idioma, Privacidade e segurança, Atualização do sistema) e conteúdo em cards (Meu PC com specs, Tela, Som, Notificações, Energia e bateria, Armazenamento) com sliders/toggles.
6. **Navegador:** abas no topo, barra de endereço, página inicial com saudação, busca, atalhos coloridos e cards "Continue de onde parou".
7. **Loja de apps:** barra lateral (Início, Apps, Jogos, Biblioteca, Downloads), banner "Destaque da semana", grade "Populares" com nota e botão Obter/Instalado.
8. **Gerenciador de arquivos (Arquivos):** barra de navegação (voltar/avançar, breadcrumb, busca), barra lateral (Início, Galeria, Documentos, Downloads, Música, Imagens), conteúdo com "Pastas" (cards) e "Recentes" (lista com data e tamanho), barra de status com contagem de itens.

## Chrome de janela
- Barra de título com ícone + nome do app à esquerda; minimizar/maximizar/fechar à direita.
- Cantos arredondados; conteúdo com padding generoso; listas com linha de seleção destacada no acento.

## Quando implementar (ordem sugerida, subprojeto 7)
Compositor/gerenciador de janelas → barra de tarefas + relógio/bandeja → menu iniciar → gerenciador de arquivos → Configurações → central de ações → tela de bloqueio/login. Tudo com toggle claro/escuro desde o início (tokens de cor, como no restante do projeto). O tema não é cosmético tardio: definir os tokens de cor (fundo, superfície, acento, texto, divisória) antes de desenhar qualquer tela.
