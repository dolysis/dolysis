# Overall Design Philosophy

This is a set of generic design ideals.

## Sections

- [Overall Design Philosophy](#overall-design-philosophy)
  - [Sections](#sections)
  - [The Big Three](#the-big-three)
    - [Simple should be Simple](#simple-should-be-simple)
    - [Complex should be Possible](#complex-should-be-possible)
    - [Transition must be Easy](#transition-must-be-easy)
  - [Other Considerations](#other-considerations)
    - [Working trumps Ideal](#working-trumps-ideal)
    - [Technical Debt is Real Debt](#technical-debt-is-real-debt)
    - [You cannot Simply by adding Complexity](#you-cannot-simply-by-adding-complexity)
    - [Never add a feature you don't need](#never-add-a-feature-you-dont-need)
    - [It is cheaper to move code than data](#it-is-cheaper-to-move-code-than-data)
    - [Security debt costs 300% more than regular debt](#security-debt-costs-300-more-than-regular-debt)

## The Big Three

To make a comparison I will use `C++` and `Perl`, this is purely to make an example and I chose these precisely to demonstrate the point and show that just because something follows these rules, that does not mean it is good or better.

### Simple should be Simple

To start and do something simple it should be easy and not require a complex configuration.

Think of Docker vs Kubernetes, where to do a simple _hello world_ container for docker under Ubuntu would be something along the lines of

```bash
# Install Docker
sudo apt install docker
sudo docker run hello-world
```

while [Install Minikube](https://matthewpalmer.net/kubernetes-app-developer/articles/install-kubernetes-ubuntu-tutorial.html) produces the following commands.

```bash
# Step 1
sudo apt-get update
sudo apt-get install -y apt-transport-https
# Step 2
sudo apt-get install -y virtualbox virtualbox-ext-pack
# Step 3
curl -s https://packages.cloud.google.com/apt/doc/apt-key.gpg | sudo apt-key add -
sudo touch /etc/apt/sources.list.d/kubernetes.list
echo "deb http://apt.kubernetes.io/ kubernetes-xenial main" | sudo tee -a /etc/apt/sources.list.d/kubernetes.list
sudo apt-get update
sudo apt-get install -y kubectl
# Step 4
curl -Lo minikube https://storage.googleapis.com/minikube/releases/v0.28.2/minikube-linux-amd64
chmod +x minikube && sudo mv minikube /usr/local/bin/
# Step 5
minikube start
kubectl api-versions
kubectl create deployment hello-minikube --image=k8s.gcr.io/echoserver:1.10
```

As you can see, that even a simple Kubernetes setup is way more complex than a Docker setup.

### Complex should be Possible

Just because it is possible to do simple things with a solution, that should not preclude you from be able to do something complex with it.

Both Docker and Kubernetes are able to handle complex environments, though there is an argument to be make that Kubernetes does a better job.

### Transition must be Easy

This means that for the transition from simple to complex most of the configuration is transferable. This is where Docker proves the point, with a simple promotional command `docker swarm init` whereas with Kubernetes it is a completely new configuration with almost nothing in common.

## Other Considerations

### Working trumps Ideal

A system that is up and running while performing adequately **is better than** an ideal solution that is down half the time, or needs special care even if it is _technically_ superior.

### Technical Debt is Real Debt

This means that anything that can be considered _technical debt_ accrues interest, and the cost in fixing it later will be worse that fixing it now.

### You cannot Simply by adding Complexity

While it is possible to abstract complexity, this is not simplification. Abstracting complexity, rather than simplifying is only beneficial when responsibility for the abstraction can be outsourced. This is _never_ cheaper than simplification.

### Never add a feature you don't need

This particularly important for supporting libraries you write. In real terms this means do not add more data abstractions than you need to make your code maintainable.

### It is cheaper to move code than data

Move data as little as possible, and move only the data you need. On the whole integers are better than floats are better than strings. All JSON/YAML/XML is strings.

### Security debt costs 300% more than regular debt

Security done early tends to iron out problems you did not know you had till you try to secure your your code.  Often to properly secure an environment the code structure need to be redesigned.
