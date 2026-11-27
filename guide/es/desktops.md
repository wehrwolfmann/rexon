[English](../en/desktops.md) | [Русский](../ru/desktops.md) | [Deutsch](../de/desktops.md) | [简体中文](../zh-CN/desktops.md) | [Français](../fr/desktops.md) | Español

# Escritorios

En esta pestaña se instalan, cambian y desinstalan entornos de escritorio.
Aquí también puedes comprobar que a tu escritorio no le falta nada, comparar
los escritorios entre sí y, con un solo clic, dar a tu escritorio el aspecto
de una distribución conocida.

![La pestaña «Escritorios»: la lista a la izquierda, «Como en una distribución» y «Herramientas» en el centro, la tarjeta del escritorio a la derecha](../images/desktops-overview.png)

## Qué hay en la pestaña

**Lista de escritorios (a la izquierda).** Los escritorios están divididos en
grupos: «Clásicos» (Cinnamon, XFCE, MATE, GNOME, KDE Plasma, Budgie, Pantheon,
Deepin, COSMIC, LXQt), «En mosaico» (Hyprland, niri, Sway, River) y
«Compositores con efectos» (Wayfire). Si un escritorio no se puede instalar en
tu distribución, la propia lista lo indica.

**Tarjeta del escritorio (a la derecha)**: todo sobre el escritorio elegido:

- capturas de pantalla (las flechas las pasan; la lupa o un clic en la imagen
  la abren a pantalla completa);
- el estado: «Instalado», «No instalado», «Escritorio actual»;
- una descripción breve y cuatro valoraciones: «Memoria» (cuánta ocupa nada
  más iniciar sesión), «Procesador», «Personalización» y «Juegos»;
- cuántos paquetes del escritorio ya están instalados, cuáles faltan y cuáles
  tienen actualizaciones;
- la versión instalada y, si la hay, una más nueva en los repositorios;
- los botones de acción:
  - **📦 Instalar**: en un escritorio que todavía no tienes;
  - **🔄 Cambiar**: cerrar la sesión e iniciarla en este escritorio;
  - **🔧 Comprobar / Reparar**: buscar e instalar los paquetes que faltan;
  - **🗑️ Desinstalar**: quitar el escritorio. El escritorio actual no tiene
    este botón;
  - **➕ Añadir la versión Wayland**: en un escritorio instalado que puede
    funcionar tanto en X11 como en Wayland, si la versión Wayland todavía no
    está instalada.
- en Hyprland, niri y Sway, el bloque **«Shells»**: paneles ya hechos con
  menú de aplicaciones y notificaciones (Noctalia, DankMaterialShell). Puedes
  instalarlos, iniciar sesión con ellos o desinstalarlos.

**🎨 Como en una distribución (en el centro).** El aspecto completo de una
distribución conocida: ahora mismo «Como en SteamOS» y «Como en Garuda Linux
(Dr460nized)». Cuando un aspecto está aplicado, aparece el botón
**↩️ Restaurar como estaba**.

**🧰 Herramientas (debajo):**

- **⚔️ Comparación de escritorios**: una tabla con todos los escritorios uno
  al lado del otro;
- **🔧 Comprobar mi escritorio**: lo mismo que «Comprobar / Reparar», pero
  para el escritorio en el que estás trabajando ahora;
- **🛟 Guardar el panel** y **↩️ Restaurar el panel**: una copia de los
  ajustes del panel por si tus pruebas con temas lo estropean. Estos botones
  solo existen en KDE Plasma, MATE y XFCE.

![Tarjeta de un escritorio instalado con los botones «Cambiar», «Comprobar / Reparar» y «Desinstalar»](../images/desktops-installed.png)

## Cómo se hace

### Instalar un entorno de escritorio

1. Elige el escritorio en la lista de la izquierda.
2. Pulsa **📦 Instalar**.
3. En la ventana «¿Instalar?», comprueba qué se va a instalar. Si el
   escritorio ofrece la opción «Cómo funcionará el escritorio», elige una
   variante (ver más abajo).
4. Confirma e introduce la contraseña de administrador cuando el sistema te
   la pida.
5. Cuando aparezca «Instalación completada», cierra la sesión y elige el nuevo
   escritorio en la pantalla de inicio de sesión. Si el escritorio necesita
   un reinicio, Rexon lo indica en la misma ventana.

### Elegir X11 o Wayland

Cinnamon, XFCE, MATE y LXQt pueden funcionar de dos formas. La elección
aparece directamente en la ventana «¿Instalar?», en la línea «Cómo funcionará
el escritorio»:

- **X11 — probado**: así funciona el escritorio desde hace años; todas las
  aplicaciones y ajustes se comportan como deben;
- **Wayland — más nuevo**: la imagen es más fluida y nítida en pantallas de
  alta resolución. En algunos escritorios todavía es experimental: parte de
  las aplicaciones, de los applets del panel y de los ajustes puede no
  funcionar;
- **Wayland — experimental, desde AUR**: así se marca el Wayland de MATE: se
  compila desde AUR en tu ordenador, y Rexon te lo advierte directamente en
  la ventana.

La variante recomendada está marcada con «(recomendado)». Si eliges Wayland,
la variante X11 también se queda: la pantalla de inicio de sesión ofrecerá
las dos. Al final, Rexon te dirá qué sesión elegir al iniciar sesión. La
versión Wayland también puede añadirse más tarde, con el botón
**➕ Añadir la versión Wayland** de la tarjeta.

### Cambiar a otro escritorio

1. Elige un escritorio instalado en la lista.
2. Pulsa **🔄 Cambiar**.
3. Guarda los documentos abiertos y confirma. Rexon guardará una copia de los
   ajustes del escritorio actual y cerrará la sesión.
4. En la pantalla de inicio de sesión, elige el nuevo escritorio.

### Desinstalar un entorno de escritorio

1. Elige el escritorio y pulsa **🗑️ Desinstalar**.
2. En la ventana «¿Eliminar?», Rexon muestra cuál es el escritorio actual: no
   se verá afectado. Confirma e introduce la contraseña.

No se puede desinstalar:

- **el escritorio actual**: de lo contrario, al siguiente inicio de sesión
  tendrías una pantalla negra. Primero cambia a otro escritorio, inicia
  sesión en él y solo entonces desinstala;
- **el último escritorio instalado**: primero instala otro.

Los paquetes compartidos con otras aplicaciones se quedan: solo se elimina
lo que ya no necesita nada más.

### Comprobar y reparar un escritorio

Si el escritorio se comporta de forma extraña (ha desaparecido el panel, no
se abren los ajustes), pulsa **🔧 Comprobar mi escritorio** en «Herramientas»
o **🔧 Comprobar / Reparar** en la tarjeta de cualquier escritorio
instalado. Rexon cotejará la lista de paquetes del escritorio e instalará
los que falten. Si la tarjeta ya indica que faltan paquetes, este botón es
el que los instala.

### Comparar escritorios

Pulsa **⚔️ Comparación de escritorios**. Se abrirá la tabla «Todos los
escritorios»: memoria, carga del procesador, en qué se basa el escritorio y
qué dibuja las ventanas. Cada fila se puede desplegar. Los botones de
«Parejas rápidas» muestran comparaciones habituales lado a lado (por ejemplo,
XFCE ↔ KDE). Para volver, usa el botón **← Escritorios**.

![Comparación de escritorios](../images/desktops-compare.png)

### Dar al escritorio el aspecto «de una distribución»

1. En el bloque **🎨 Como en una distribución**, pulsa **Aplicar** en el
   aspecto que quieras.
2. Rexon comprueba qué hace falta y muestra una ventana: qué cambiará, qué se
   aplicará solo en parte en tu escritorio y qué no se aplicará en absoluto.
   En el aspecto de SteamOS puedes elegir el estilo y, si quieres, activar el
   modo de juego.
3. Pulsa **Aplicar**. Si el aspecto necesita aplicaciones, Rexon las instala
   y te pide la contraseña.
4. ¿Has cambiado de idea? Pulsa **↩️ Restaurar como estaba**: vuelven el
   aspecto y los ajustes anteriores. Las aplicaciones instaladas para el
   aspecto se quedan; puedes desinstalarlas tú mismo.

![La ventana «¿Aplicar «Como en SteamOS»?» con la lista de cambios](../images/desktops-looks.png)

## Si algo sale mal

- **Después de instalarlo, el escritorio no aparece en la pantalla de inicio
  de sesión.** Cierra la sesión otra vez. Si Rexon ha indicado que hace falta
  reiniciar, reinicia el ordenador. En Cinnamon, XFCE, MATE y LXQt, comprueba
  qué sesión (X11 o Wayland) estás eligiendo.
- **El escritorio arranca sin panel o con errores.** Inicia sesión en
  cualquier otro escritorio, o quédate en este, y pulsa
  **🔧 Comprobar mi escritorio**.
- **Un tema ha estropeado el panel.** Si antes pulsaste
  **🛟 Guardar el panel**, pulsa **↩️ Restaurar el panel**.
- **No te gusta el aspecto «de una distribución».** Usa el botón
  **↩️ Restaurar como estaba** de su recuadro.
- **La ventana «Un paquete ya no recibe actualizaciones».** En el sistema ha
  quedado un paquete que se ha retirado de los repositorios. Si no lo usas,
  Rexon puede desinstalarlo; si no, pulsa «Ahora no».
- **«No se ha detectado el escritorio».** Rexon no ha reconocido el
  escritorio en el que trabajas. Elígelo en la lista de la izquierda y pulsa
  **🔧 Comprobar / Reparar** en su tarjeta.
- **Hay que revertir todo el sistema.** Rexon no hace instantáneas del
  sistema. Si tienes configurado snapper con snap-pac (como suele ocurrir en
  CachyOS y otras distribuciones sobre Btrfs), se crea automáticamente una
  instantánea antes de cada instalación y desinstalación de paquetes, y
  puedes volver a ella con las herramientas de snapper (por ejemplo, con
  Btrfs Assistant).
- **Después de cambiar ha desaparecido el panel de KDE Plasma.** Antes de
  cambiar desde Plasma, Rexon guarda su panel; puedes recuperarlo en
  **Ajustes → 🛡️ Protección del panel de KDE Plasma**. Si el nuevo escritorio
  se ha quedado con el navegador o el reproductor predeterminado, mira en
  **Ajustes → 🔒 Protección de aplicaciones predeterminadas**.

## Véase también

- [Apariencia](appearance.md): temas, iconos, cursores, tipos de letra,
  pantalla de inicio de sesión
- [Extensiones](extensions.md): paquetes `.rxn` y temas de Windows
- [Wallpaper Engine](wallpaper-engine.md): fondos de pantalla animados
- [Steam](steam.md): vídeo de inicio al iniciar sesión y fondos del perfil
- [Ajustes](settings.md): copias de seguridad y opciones de Rexon
- [Índice de la guía](../README.md)
